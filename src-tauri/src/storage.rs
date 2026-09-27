//! 存储位置：图片、数据库、软件数据、缓存四类目录，用户可以分别修改。
//!
//! 位置设置写在系统配置目录下固定的 `storage.json`，软件启动时靠它找到其余目录，
//! 所以这个文件本身的位置不能改。
//! - 图片、缓存：修改后立即生效。
//! - 数据库、软件数据：运行中文件一直被占用，先记为待迁移，下次启动时先搬再打开。
//! - 默认的数据库目录是「软件数据目录/database」，跟着软件数据一起走。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::i18n::{text, tr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StorageKind {
    Images,
    Database,
    Data,
    Cache,
}

impl StorageKind {
    pub const ALL: [StorageKind; 4] = [StorageKind::Images, StorageKind::Database, StorageKind::Data, StorageKind::Cache];

    /// 错误信息里用的名称。
    pub fn label(self) -> &'static str {
        match self {
            StorageKind::Images => text("图片", "Images"),
            StorageKind::Database => text("数据库", "Database"),
            StorageKind::Data => text("软件数据", "App data"),
            StorageKind::Cache => text("缓存", "Cache"),
        }
    }

    /// 运行中文件一直被占用，需要重启后才能迁移。
    pub fn applies_on_restart(self) -> bool {
        matches!(self, StorageKind::Database | StorageKind::Data)
    }
}

/// 修改位置时怎么处理旧位置里的内容。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeMode {
    /// 把已有内容移到新位置（新位置必须是空文件夹）。
    Move,
    /// 不移动：图片留在原处；缓存直接清空；数据库和软件数据改用新位置里已有的内容，没有就从空开始。
    Leave,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingChange {
    pub kind: StorageKind,
    /// `None` 表示恢复默认位置。
    pub to: Option<PathBuf>,
    pub mode: ChangeMode,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StorageFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    images: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    database: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    data: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cache: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pending: Vec<PendingChange>,
    /// 上次启动时迁移失败的原因，用户在界面上关闭提示后清除。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Defaults {
    pub images: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    NotAbsolute,
    Same,
    Nested,
    /// 和另一类内容的位置重叠。
    Overlap(StorageKind),
    TargetNotEmpty,
    NotWritable(String),
    Move(String),
    Save(String),
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            StorageError::NotAbsolute => tr!("请选择完整的文件夹路径", "Choose a full folder path"),
            StorageError::Same => tr!("新位置和当前位置相同", "The new location is the same as the current one"),
            StorageError::Nested => tr!(
                "新位置不能在当前位置里面，当前位置也不能在新位置里面",
                "The new location can't be inside the current one, or the other way around"
            ),
            StorageError::Overlap(kind) => {
                let label = kind.label();
                tr!("新位置不能和「{label}」的位置重叠", "The new location can't overlap the “{label}” location")
            }
            StorageError::TargetNotEmpty => tr!(
                "新位置已有文件，移动时请选一个空文件夹",
                "The new location already contains files. Choose an empty folder to move into"
            ),
            StorageError::NotWritable(detail) => {
                tr!("无法写入新位置：{detail}", "Can't write to the new location: {detail}")
            }
            StorageError::Move(detail) => tr!("移动失败：{detail}", "Moving failed: {detail}"),
            StorageError::Save(detail) => {
                tr!("保存位置设置失败：{detail}", "Couldn't save the location settings: {detail}")
            }
        };
        f.write_str(&message)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationInfo {
    pub kind: StorageKind,
    pub path: String,
    pub default_path: String,
    pub is_default: bool,
    pub has_data: bool,
    pub applies_on_restart: bool,
    pub pending: Option<PendingInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingInfo {
    pub to: String,
    pub mode: ChangeMode,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageInfo {
    pub locations: Vec<LocationInfo>,
    pub config_file: String,
    pub last_error: Option<String>,
}

pub struct Storage {
    file: PathBuf,
    defaults: Defaults,
    state: StorageFile,
}

/// 一次已通过校验的位置修改。
#[derive(Debug)]
pub struct ChangePlan {
    pub kind: StorageKind,
    from: PathBuf,
    to: PathBuf,
    mode: ChangeMode,
    /// 写进设置的值；`None` 表示默认位置。
    target: Option<PathBuf>,
}

impl ChangePlan {
    /// 按计划移动或清理旧位置里的内容。可能耗时较长，不要在持锁时调用。
    pub fn execute(&self) -> Result<(), StorageError> {
        apply_move(self.kind, &self.from, &self.to, self.mode)
    }

    /// 整体移动时返回（旧位置，新位置），用来改写图库里记录的文件路径。
    pub fn moved(&self) -> Option<(&Path, &Path)> {
        (self.mode == ChangeMode::Move).then_some((self.from.as_path(), self.to.as_path()))
    }
}

impl Storage {
    /// 读取位置设置；文件缺失或损坏时按默认位置处理。
    pub fn load(file: PathBuf, defaults: Defaults) -> Self {
        let state = fs::read(&file).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
        Self { file, defaults, state }
    }

    fn configured(&self, kind: StorageKind) -> Option<&PathBuf> {
        match kind {
            StorageKind::Images => self.state.images.as_ref(),
            StorageKind::Database => self.state.database.as_ref(),
            StorageKind::Data => self.state.data.as_ref(),
            StorageKind::Cache => self.state.cache.as_ref(),
        }
    }

    fn set_configured(&mut self, kind: StorageKind, value: Option<PathBuf>) {
        let slot = match kind {
            StorageKind::Images => &mut self.state.images,
            StorageKind::Database => &mut self.state.database,
            StorageKind::Data => &mut self.state.data,
            StorageKind::Cache => &mut self.state.cache,
        };
        *slot = value;
    }

    pub fn default_path(&self, kind: StorageKind) -> PathBuf {
        match kind {
            StorageKind::Images => self.defaults.images.clone(),
            StorageKind::Data => self.defaults.data.clone(),
            StorageKind::Cache => self.defaults.cache.clone(),
            StorageKind::Database => self.path(StorageKind::Data).join("database"),
        }
    }

    pub fn path(&self, kind: StorageKind) -> PathBuf {
        self.configured(kind).cloned().unwrap_or_else(|| self.default_path(kind))
    }

    pub fn is_default(&self, kind: StorageKind) -> bool {
        self.configured(kind).is_none()
    }

    pub fn config_file(&self) -> &Path {
        &self.file
    }

    /// 创建软件运行必需的目录。图片目录在第一次下载时再创建，不提前在用户的图片文件夹里建目录。
    pub fn ensure_runtime_dirs(&self) -> io::Result<()> {
        for kind in [StorageKind::Data, StorageKind::Database, StorageKind::Cache] {
            fs::create_dir_all(self.path(kind))?;
        }
        Ok(())
    }

    pub fn prepare(&self, kind: StorageKind) -> io::Result<PathBuf> {
        let path = self.path(kind);
        fs::create_dir_all(&path)?;
        Ok(path)
    }

    pub fn info(&self) -> StorageInfo {
        let locations = StorageKind::ALL
            .into_iter()
            .map(|kind| {
                let path = self.path(kind);
                let pending = self.state.pending.iter().find(|p| p.kind == kind).map(|p| PendingInfo {
                    to: p.to.clone().unwrap_or_else(|| self.default_path(kind)).display().to_string(),
                    mode: p.mode,
                });
                LocationInfo {
                    kind,
                    path: path.display().to_string(),
                    default_path: self.default_path(kind).display().to_string(),
                    is_default: self.is_default(kind),
                    has_data: has_entries(&path),
                    applies_on_restart: kind.applies_on_restart(),
                    pending,
                }
            })
            .collect();
        StorageInfo {
            locations,
            config_file: self.file.display().to_string(),
            last_error: self.state.last_error.clone(),
        }
    }

    pub fn dismiss_error(&mut self) -> Result<(), StorageError> {
        self.state.last_error = None;
        self.save()
    }

    /// 校验并生成迁移计划。`target` 为 `None` 表示恢复默认。
    /// 计划不持有锁：调用方可以在锁外执行耗时的移动，再用 [`Storage::commit`] 写回设置。
    pub fn plan(&self, kind: StorageKind, target: Option<PathBuf>, mode: ChangeMode) -> Result<ChangePlan, StorageError> {
        let from = self.path(kind);
        let to = target.clone().unwrap_or_else(|| self.default_path(kind));
        self.validate(kind, &from, &to, mode, target.is_none())?;
        let target = self.canonical_target(kind, target);
        Ok(ChangePlan { kind, from, to, mode, target })
    }

    /// 写回设置。数据库、软件数据记为待迁移并返回 `false`（重启后生效）；
    /// 其余类型须先执行 [`ChangePlan::execute`]，返回 `true`（已生效）。
    pub fn commit(&mut self, plan: &ChangePlan) -> Result<bool, StorageError> {
        if plan.kind.applies_on_restart() {
            self.state.pending.retain(|p| p.kind != plan.kind);
            self.state.pending.push(PendingChange { kind: plan.kind, to: plan.target.clone(), mode: plan.mode });
            self.save()?;
            return Ok(false);
        }
        self.set_configured(plan.kind, plan.target.clone());
        self.save()?;
        Ok(true)
    }

    /// 校验、执行并写回，供不需要在锁外移动的场景使用。
    pub fn change(&mut self, kind: StorageKind, target: Option<PathBuf>, mode: ChangeMode) -> Result<bool, StorageError> {
        let plan = self.plan(kind, target, mode)?;
        if !kind.applies_on_restart() {
            plan.execute()?;
        }
        self.commit(&plan)
    }

    pub fn cancel_pending(&mut self, kind: StorageKind) -> Result<(), StorageError> {
        self.state.pending.retain(|p| p.kind != kind);
        self.save()
    }

    /// 启动时执行上次记下的迁移，必须在打开数据库、读取软件数据之前调用。
    /// 失败的项目保持原位置，原因记进 `last_error` 供界面提示。
    pub fn apply_pending(&mut self) {
        if self.state.pending.is_empty() {
            return;
        }
        let mut pending = std::mem::take(&mut self.state.pending);
        // 默认数据库在软件数据目录里面，先处理数据库，免得它被软件数据的迁移一并带走。
        pending.sort_by_key(|p| if p.kind == StorageKind::Database { 0 } else { 1 });
        let mut errors = Vec::new();
        for change in pending {
            let from = self.path(change.kind);
            let to = change.to.clone().unwrap_or_else(|| self.default_path(change.kind));
            let result = if normalize(&from) == normalize(&to) {
                Ok(())
            } else {
                self.validate(change.kind, &from, &to, change.mode, change.to.is_none())
                    .and_then(|_| apply_move(change.kind, &from, &to, change.mode))
            };
            match result {
                Ok(()) => {
                    let target = self.canonical_target(change.kind, change.to);
                    self.set_configured(change.kind, target);
                }
                Err(err) => {
                    let label = change.kind.label();
                    errors.push(tr!("{label}：{err}", "{label}: {err}"));
                }
            }
        }
        self.state.last_error = (!errors.is_empty()).then(|| {
            let errors = errors.join(text("；", "; "));
            tr!(
                "上次启动时迁移失败，位置保持不变。{errors}",
                "Moving failed at the last launch, so the locations stayed unchanged. {errors}"
            )
        });
        let _ = self.save();
    }

    /// 用户手动选中的文件夹恰好是默认位置时，按默认位置保存。
    fn canonical_target(&self, kind: StorageKind, target: Option<PathBuf>) -> Option<PathBuf> {
        target.filter(|path| normalize(path) != normalize(&self.default_path(kind)))
    }

    fn validate(
        &self,
        kind: StorageKind,
        from: &Path,
        to: &Path,
        mode: ChangeMode,
        resetting: bool,
    ) -> Result<(), StorageError> {
        if !to.is_absolute() {
            return Err(StorageError::NotAbsolute);
        }
        let (from_n, to_n) = (normalize(from), normalize(to));
        if from_n == to_n {
            return Err(StorageError::Same);
        }
        if to_n.starts_with(&from_n) || from_n.starts_with(&to_n) {
            return Err(StorageError::Nested);
        }
        for other in StorageKind::ALL.into_iter().filter(|&k| k != kind) {
            // 默认数据库本来就在软件数据目录里：移动软件数据时它会一起搬走，恢复数据库默认位置也会回到这里。
            let expected_nesting = (kind == StorageKind::Data && other == StorageKind::Database && self.is_default(other))
                || (kind == StorageKind::Database && other == StorageKind::Data && resetting);
            if expected_nesting {
                continue;
            }
            let other_n = normalize(&self.path(other));
            if to_n == other_n || to_n.starts_with(&other_n) || other_n.starts_with(&to_n) {
                return Err(StorageError::Overlap(other));
            }
        }
        if mode == ChangeMode::Move && has_entries(&to_n) {
            return Err(StorageError::TargetNotEmpty);
        }
        Ok(())
    }

    fn save(&self) -> Result<(), StorageError> {
        let save = || -> io::Result<()> {
            if let Some(parent) = self.file.parent() {
                fs::create_dir_all(parent)?;
            }
            let bytes = serde_json::to_vec_pretty(&self.state).map_err(io::Error::other)?;
            let temp = self.file.with_extension("json.part");
            fs::write(&temp, bytes)?;
            fs::rename(&temp, &self.file)
        };
        save().map_err(|e| StorageError::Save(e.to_string()))
    }
}

fn apply_move(kind: StorageKind, from: &Path, to: &Path, mode: ChangeMode) -> Result<(), StorageError> {
    fs::create_dir_all(to).map_err(|e| StorageError::NotWritable(e.to_string()))?;
    check_writable(to)?;
    match mode {
        ChangeMode::Move => {
            move_contents(from, to).map_err(|e| StorageError::Move(e.to_string()))?;
            // 旧目录空了就删掉；不为空（例如其中还有别的目录）就保留。
            let _ = fs::remove_dir(from);
        }
        ChangeMode::Leave if kind == StorageKind::Cache => {
            let _ = remove_contents(from);
        }
        ChangeMode::Leave => {}
    }
    Ok(())
}

fn check_writable(dir: &Path) -> Result<(), StorageError> {
    let probe = dir.join(format!(".imagebox-write-test-{}", std::process::id()));
    fs::write(&probe, b"ok").and_then(|_| fs::remove_file(&probe)).map_err(|e| StorageError::NotWritable(e.to_string()))
}

fn has_entries(dir: &Path) -> bool {
    fs::read_dir(dir).map(|mut entries| entries.next().is_some()).unwrap_or(false)
}

/// 取最近一个已存在的上级目录做规范化，再拼回其余部分，
/// 这样还不存在的新位置也能和现有目录正确比较（符号链接、大小写、`..`）。
pub fn normalize(path: &Path) -> PathBuf {
    let mut existing = path;
    let mut rest = Vec::new();
    loop {
        if let Ok(canonical) = fs::canonicalize(existing) {
            return rest.iter().rev().fold(canonical, |acc, part| acc.join(part));
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_os_string());
                existing = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// 把 `from` 里的内容移到 `to`。同一磁盘直接重命名；跨磁盘时复制并核对大小后再删除源文件。
pub fn move_contents(from: &Path, to: &Path) -> io::Result<()> {
    let entries = match fs::read_dir(from) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    fs::create_dir_all(to)?;
    for entry in entries {
        let entry = entry?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        match fs::rename(&source, &target) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::CrossesDevices => {
                copy_recursive(&source, &target)?;
                remove_path(&source)?;
            }
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

fn copy_recursive(source: &Path, target: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(source)?;
    if meta.is_dir() {
        fs::create_dir_all(target)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &target.join(entry.file_name()))?;
        }
        return Ok(());
    }
    let copied = fs::copy(source, target)?;
    if copied != fs::metadata(source)?.len() {
        let path = source.display();
        return Err(io::Error::other(tr!("{path} 复制后大小不一致", "{path} has a different size after copying")));
    }
    Ok(())
}

fn remove_path(path: &Path) -> io::Result<()> {
    if fs::symlink_metadata(path)?.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

fn remove_contents(dir: &Path) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        remove_path(&entry?.path())?;
    }
    Ok(())
}

/// 把文件移到系统的废纸篓（Windows 上是回收站），用户还能找回。文件已经不在时算成功。
/// macOS 上用 NSFileManager，不需要「控制访达」的权限，也不会弹授权窗口。
pub fn move_to_trash(path: &Path) -> Result<(), String> {
    if fs::symlink_metadata(path).is_err() {
        return Ok(());
    }
    #[allow(unused_mut)]
    let mut trash = trash::TrashContext::default();
    #[cfg(target_os = "macos")]
    {
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        trash.set_delete_method(DeleteMethod::NsFileManager);
    }
    trash.delete(path).map_err(|e| e.to_string())
}

/// 目录占用的字节数；读不到的文件跳过。
pub fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(path) else { return 0 };
    entries
        .flatten()
        .map(|entry| match entry.metadata() {
            Ok(meta) if meta.is_dir() => dir_size(&entry.path()),
            Ok(meta) => meta.len(),
            Err(_) => 0,
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        _root: tempfile::TempDir,
        base: PathBuf,
        storage: Storage,
    }

    fn fixture() -> Fixture {
        let root = tempfile::tempdir().unwrap();
        let base = normalize(root.path());
        let defaults = Defaults { images: base.join("Pictures/YPuddinImageBox"), data: base.join("app/data"), cache: base.join("cache/image-cache") };
        let storage = Storage::load(base.join("app/storage.json"), defaults);
        storage.ensure_runtime_dirs().unwrap();
        Fixture { _root: root, base, storage }
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn defaults_and_derived_database_path() {
        let f = fixture();
        assert_eq!(f.storage.path(StorageKind::Database), f.base.join("app/data/database"));
        assert!(StorageKind::ALL.iter().all(|&k| f.storage.is_default(k)));
    }

    #[test]
    fn moves_cache_immediately_and_persists() {
        let mut f = fixture();
        write(&f.storage.path(StorageKind::Cache).join("remote/ab/cd"), "img");
        let target = f.base.join("big-disk/cache");
        assert!(f.storage.change(StorageKind::Cache, Some(target.clone()), ChangeMode::Move).unwrap());
        assert_eq!(fs::read_to_string(target.join("remote/ab/cd")).unwrap(), "img");
        assert!(!f.base.join("cache/image-cache").exists());
        let reloaded = Storage::load(f.base.join("app/storage.json"), f.storage.defaults.clone());
        assert_eq!(reloaded.path(StorageKind::Cache), target);
    }

    #[test]
    fn leave_mode_clears_cache_but_keeps_images() {
        let mut f = fixture();
        write(&f.storage.path(StorageKind::Cache).join("a"), "x");
        f.storage.change(StorageKind::Cache, Some(f.base.join("c2")), ChangeMode::Leave).unwrap();
        assert!(!has_entries(&f.base.join("cache/image-cache")));

        let images = f.storage.path(StorageKind::Images);
        write(&images.join("danbooru/1.jpg"), "x");
        f.storage.change(StorageKind::Images, Some(f.base.join("i2")), ChangeMode::Leave).unwrap();
        assert!(images.join("danbooru/1.jpg").exists());
    }

    #[test]
    fn rejects_unsafe_targets() {
        let mut f = fixture();
        let cache = f.storage.path(StorageKind::Cache);
        let data = f.storage.path(StorageKind::Data);
        let err = |r: Result<bool, StorageError>| r.unwrap_err().to_string();
        assert!(err(f.storage.change(StorageKind::Cache, Some(PathBuf::from("relative")), ChangeMode::Move)).contains("完整"));
        assert!(err(f.storage.change(StorageKind::Cache, Some(cache.clone()), ChangeMode::Move)).contains("相同"));
        assert!(err(f.storage.change(StorageKind::Cache, Some(cache.join("sub")), ChangeMode::Move)).contains("里面"));
        assert!(err(f.storage.change(StorageKind::Images, Some(data.join("pics")), ChangeMode::Move)).contains("软件数据"));
        let busy = f.base.join("busy");
        write(&busy.join("file"), "x");
        assert!(err(f.storage.change(StorageKind::Cache, Some(busy.clone()), ChangeMode::Move)).contains("空文件夹"));
        // 不移动时可以指向已有内容的文件夹。
        assert!(f.storage.change(StorageKind::Images, Some(busy), ChangeMode::Leave).is_ok());
    }

    #[test]
    fn database_change_waits_for_restart() {
        let mut f = fixture();
        let db = f.storage.path(StorageKind::Database);
        write(&db.join("library.sqlite"), "db");
        let target = f.base.join("ssd/db");
        assert!(!f.storage.change(StorageKind::Database, Some(target.clone()), ChangeMode::Move).unwrap());
        assert!(db.join("library.sqlite").exists(), "运行中不移动");
        assert!(f.storage.info().locations.iter().any(|l| l.kind == StorageKind::Database && l.pending.is_some()));

        let mut next = Storage::load(f.base.join("app/storage.json"), f.storage.defaults.clone());
        next.apply_pending();
        assert_eq!(next.path(StorageKind::Database), target);
        assert_eq!(fs::read_to_string(target.join("library.sqlite")).unwrap(), "db");
        assert!(next.info().last_error.is_none());
    }

    #[test]
    fn default_database_moves_with_data() {
        let mut f = fixture();
        write(&f.storage.path(StorageKind::Database).join("library.sqlite"), "db");
        write(&f.storage.path(StorageKind::Data).join("settings.json"), "{}");
        let target = f.base.join("elsewhere/data");
        f.storage.change(StorageKind::Data, Some(target.clone()), ChangeMode::Move).unwrap();
        let mut next = Storage::load(f.base.join("app/storage.json"), f.storage.defaults.clone());
        next.apply_pending();
        assert_eq!(next.path(StorageKind::Database), target.join("database"));
        assert!(target.join("database/library.sqlite").exists());
        assert!(target.join("settings.json").exists());
    }

    #[test]
    fn failed_pending_move_keeps_old_location() {
        let mut f = fixture();
        let target = f.base.join("ssd/db");
        f.storage.change(StorageKind::Database, Some(target.clone()), ChangeMode::Move).unwrap();
        // 重启前新位置被放进了文件，迁移应当放弃并提示。
        write(&target.join("other"), "x");
        let mut next = Storage::load(f.base.join("app/storage.json"), f.storage.defaults.clone());
        next.apply_pending();
        assert!(next.is_default(StorageKind::Database));
        assert!(next.info().last_error.unwrap().contains("数据库"));
    }

    #[test]
    fn picking_default_folder_counts_as_default() {
        let mut f = fixture();
        let moved = f.base.join("c2");
        f.storage.change(StorageKind::Cache, Some(moved), ChangeMode::Leave).unwrap();
        let default = f.storage.default_path(StorageKind::Cache);
        f.storage.change(StorageKind::Cache, Some(default), ChangeMode::Leave).unwrap();
        assert!(f.storage.is_default(StorageKind::Cache));
    }
}

#[cfg(test)]
mod trash_tests {
    use super::*;

    /// 真的把一个临时文件移进废纸篓再清掉，平时跳过：`cargo test trash -- --ignored`。
    #[test]
    #[ignore = "会往系统废纸篓里放一个临时文件，需要手动运行"]
    fn moves_file_to_system_trash() {
        let dir = tempfile::tempdir().unwrap();
        let name = format!("imagebox-trash-test-{}.txt", std::process::id());
        let file = dir.path().join(&name);
        fs::write(&file, b"test").unwrap();
        move_to_trash(&file).unwrap();
        assert!(!file.exists());
        // 已经不在的文件算成功。
        move_to_trash(&file).unwrap();
        #[cfg(target_os = "macos")]
        if let Some(home) = std::env::var_os("HOME") {
            let _ = fs::remove_file(PathBuf::from(home).join(".Trash").join(&name));
        }
    }
}
