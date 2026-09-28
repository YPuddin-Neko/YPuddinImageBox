import { useEffect, useState, type FormEvent } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";

import { Icon } from "../../components/Icon";
import { Toast, useToast } from "../../components/Toast";
import { t, type Msg } from "../../lib/i18n";
import { errorMessage, SOURCE_LABEL, type Source } from "../../lib/ipc";
import { isMac } from "../../lib/platform";
import {
  accountKeyStorage,
  accountRemove,
  accountSave,
  accountsInfo,
  type AccountsInfo,
  type AccountView,
  type KeyStorage,
} from "../../lib/settings";

interface SiteText {
  nameLabel: Msg;
  namePlaceholder: Msg;
  description: Msg;
  help: Msg;
  helpUrl: string;
  helpLink: Msg;
}

/** 要填账号的站点。不用登录的站点（Yande.re）不在这里，也不会出现在账号列表里。 */
const SITES: Partial<Record<Source, SiteText>> = {
  danbooru: {
    nameLabel: "用户名",
    namePlaceholder: "Danbooru 用户名",
    description: "不登录也能浏览和下载。Gold 以上等级的账号一次能搜更多 tag（Gold 6 个、Platinum 12 个），带受限 tag 的图也只有 Gold 以上能下载原图。",
    help: "登录 Danbooru 网页后，在「My Account」页面的 API Key 一栏创建。",
    helpUrl: "https://danbooru.donmai.us/profile",
    helpLink: "打开 Danbooru 账号页",
  },
  gelbooru: {
    nameLabel: "User ID",
    namePlaceholder: "数字 ID",
    description: "Gelbooru 的接口必须填写账号才能使用。",
    help: "登录 Gelbooru 网页后，在「My Account → Options」页面底部的 API Access Credentials 里。",
    helpUrl: "https://gelbooru.com/index.php?page=account&s=options",
    helpLink: "打开 Gelbooru 设置页",
  },
};

const keychain = () => (isMac ? t("钥匙串") : t("Windows 凭据管理器"));

const storageOptions = (): { value: KeyStorage; title: string; description: string }[] => [
  {
    value: "keychain",
    title: isMac ? t("系统钥匙串") : t("凭据管理器"),
    description: t("交给系统的{keychain}保管，最安全，推荐使用。", { keychain: keychain() }),
  },
  {
    value: "file",
    title: t("设置文件"),
    description: t("用这台电脑专属的密钥加密后存在设置文件里；文件被复制到别的电脑上解不开，换电脑需要重新填写。"),
  },
];

function AccountCard({
  site,
  account,
  onChange,
  onNotice,
}: {
  site: SiteText;
  account: AccountView;
  onChange: (info: AccountsInfo) => void;
  onNotice: (message: string) => void;
}) {
  const label = SOURCE_LABEL[account.source];
  const signedIn = account.name !== null && !account.keyMissing;
  const [editing, setEditing] = useState(!signedIn);
  const [name, setName] = useState(account.name ?? "");
  const [apiKey, setApiKey] = useState("");
  const [busy, setBusy] = useState<"save" | "remove" | null>(null);
  const [error, setError] = useState<string | null>(null);

  const save = async (event: FormEvent) => {
    event.preventDefault();
    setBusy("save");
    setError(null);
    try {
      onChange(await accountSave(account.source, name, apiKey));
      setApiKey("");
      setEditing(false);
      onNotice(t("已登录 {site}", { site: label }));
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(null);
    }
  };

  const remove = async () => {
    setBusy("remove");
    setError(null);
    try {
      onChange(await accountRemove(account.source));
      setName("");
      setEditing(true);
      onNotice(t("已退出 {site}", { site: label }));
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(null);
    }
  };

  const status = signedIn ? (
    <span className="badge ok">{t("已登录")}</span>
  ) : account.keyMissing ? (
    <span className="badge warn">{t("需要重新填写 API Key")}</span>
  ) : (
    <span className="badge">{t("未登录")}</span>
  );

  return (
    <section className="set-card" aria-labelledby={`account-${account.source}`}>
      <div className="set-title">
        <h2 id={`account-${account.source}`}>{label}</h2>
        {status}
      </div>
      <p className="set-desc">{t(site.description)}</p>

      {signedIn && !editing ? (
        <div className="set-line">
          <div className="identity">
            <Icon name="user" size={15} />
            <b>{account.name}</b>
            {account.level && <span className="badge">{account.level}</span>}
          </div>
          <div className="set-actions">
            <button type="button" className="btn" onClick={() => setEditing(true)} disabled={busy !== null}>
              {t("更换账号")}
            </button>
            <button type="button" className="btn ghost" onClick={() => void remove()} disabled={busy !== null}>
              {busy === "remove" ? t("正在退出…") : t("退出登录")}
            </button>
          </div>
        </div>
      ) : (
        <form className="form-row" onSubmit={(event) => void save(event)}>
          <label className="field">
            <span>{t(site.nameLabel)}</span>
            <input
              className="field-input"
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder={t(site.namePlaceholder)}
              autoComplete="off"
              spellCheck={false}
            />
          </label>
          <label className="field">
            <span>API Key</span>
            <input
              className="field-input"
              type="password"
              value={apiKey}
              onChange={(event) => setApiKey(event.target.value)}
              placeholder={t("粘贴 API Key")}
              autoComplete="off"
              spellCheck={false}
            />
          </label>
          <div className="set-actions">
            <button type="submit" className="btn primary" disabled={busy !== null}>
              {busy === "save" ? t("正在验证…") : t("保存并验证")}
            </button>
            {signedIn && (
              <button type="button" className="btn ghost" onClick={() => setEditing(false)} disabled={busy !== null}>
                {t("取消")}
              </button>
            )}
            {account.keyMissing && (
              <button type="button" className="btn ghost" onClick={() => void remove()} disabled={busy !== null}>
                {t("退出登录")}
              </button>
            )}
          </div>
        </form>
      )}

      {error && <p className="form-error">{error}</p>}
      {(!signedIn || editing) && (
        <p className="form-hint">
          <span>{t(site.help)}</span>
          <button type="button" className="link" onClick={() => void openUrl(site.helpUrl)}>
            {t(site.helpLink)}
          </button>
        </p>
      )}
    </section>
  );
}

export function AccountsSettings() {
  const [info, setInfo] = useState<AccountsInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [switching, setSwitching] = useState(false);
  const [notice, setNotice] = useToast();

  useEffect(() => {
    accountsInfo().then(setInfo, (err) => setError(errorMessage(err)));
  }, []);

  const changeStorage = async (storage: KeyStorage) => {
    setSwitching(true);
    setError(null);
    try {
      setInfo(await accountKeyStorage(storage));
      setNotice(
        storage === "file"
          ? t("API Key 已改为加密保存在设置文件里")
          : t("API Key 已改为保存在系统{keychain}里", { keychain: keychain() }),
      );
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setSwitching(false);
    }
  };

  const where = info?.keyStorage === "file" ? t("加密后存在设置文件里") : t("存在系统{keychain}里", { keychain: keychain() });

  return (
    <div className="settings-page">
      <header className="settings-head">
        <h1>{t("账号")}</h1>
        <p>{t("保存前会先用填写的账号访问一次站点。API Key {where}。", { where })}</p>
      </header>

      {(error ?? info?.error) && (
        <div className="alert" role="alert">
          <span>{error ?? t("读取 API Key 失败：{error}", { error: info?.error ?? "" })}</span>
        </div>
      )}

      <div className="set-list">
        {info?.accounts.map((account) => {
          const site = SITES[account.source];
          return (
            site && (
              <AccountCard
                key={`${account.source}-${account.name ?? ""}-${account.keyMissing}`}
                site={site}
                account={account}
                onChange={setInfo}
                onNotice={setNotice}
              />
            )
          );
        })}

        {info && (
          <section className="set-card" aria-labelledby="key-storage-title">
            <div className="set-title">
              <h2 id="key-storage-title">{t("API Key 的保存方式")}</h2>
            </div>
            <p className="set-desc">{t("切换后，已经保存的 API Key 会一起搬过去。")}</p>
            <div className="options" role="radiogroup" aria-labelledby="key-storage-title">
              {storageOptions().map((option) => (
                <label key={option.value} className="option" data-checked={info.keyStorage === option.value || undefined}>
                  <input
                    type="radio"
                    name="key-storage"
                    checked={info.keyStorage === option.value}
                    disabled={switching}
                    onChange={() => void changeStorage(option.value)}
                  />
                  <span className="option-title">{option.title}</span>
                  <span className="option-desc">{option.description}</span>
                </label>
              ))}
            </div>
          </section>
        )}
      </div>

      <Toast message={notice} />
    </div>
  );
}
