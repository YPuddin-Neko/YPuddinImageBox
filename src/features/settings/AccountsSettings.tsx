import { useEffect, useRef, useState, type FormEvent } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";

import { Icon } from "../../components/Icon";
import { SwapLabel } from "../../components/SwapLabel";
import { Toast, useToast } from "../../components/Toast";
import { t, type Msg } from "../../lib/i18n";
import { errorMessage, SOURCE_LABEL, type Source } from "../../lib/ipc";
import { isMac } from "../../lib/platform";
import {
  accountKeyStorage,
  accountRemove,
  accountSave,
  accountsInfo,
  pixivLoginCheck,
  pixivLoginOpen,
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

/** 填用户名和 API Key 的站点。Pixiv 用自己的卡片（见 PixivCard）；不用登录的站点（Yande.re）不在账号列表里。 */
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
  e621: {
    nameLabel: "用户名",
    namePlaceholder: "e621 用户名",
    description: "不登录也能搜索和下载公开图片。填写账号和 API Key 后，接口会使用登录身份访问。",
    help: "登录 e621 后，在「Account」页面创建 API Key。",
    helpUrl: "https://e621.net/account",
    helpLink: "打开 e621 账号页",
  },
  rule34: {
    nameLabel: "User ID",
    namePlaceholder: "数字 ID",
    description: "Rule34.xxx 的接口必须填写 User ID 和 API Key 才能搜索。",
    help: "登录 Rule34.xxx 后，在账号设置页生成 API Access Credentials。",
    helpUrl: "https://rule34.xxx/index.php?page=account&s=options",
    helpLink: "打开 Rule34.xxx 设置页",
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

/** 登录窗口开着时，隔多久问一次登录好了没有。 */
const LOGIN_POLL_MS = 1500;

/**
 * Pixiv 没有 API Key：推荐在系统浏览器里登录后粘贴 PHPSESSID；也可以在软件窗口里登录。
 */
function PixivCard({
  account,
  onChange,
  onNotice,
}: {
  account: AccountView;
  onChange: (info: AccountsInfo) => void;
  onNotice: (message: string) => void;
}) {
  const label = SOURCE_LABEL[account.source];
  const signedIn = account.name !== null && !account.keyMissing;
  const [session, setSession] = useState("");
  const [busy, setBusy] = useState<"save" | "remove" | null>(null);
  const [waiting, setWaiting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // 每次打开登录窗口加一，重新开始等待。
  const [round, setRound] = useState(0);
  const latest = useRef({ onChange, onNotice });
  latest.current = { onChange, onNotice };

  // 登录窗口开着时隔一会儿问一次。打开这一页时也问一次：离开时窗口还开着的话接着等。
  useEffect(() => {
    if (signedIn) return;
    let stopped = false;
    let timer = 0;
    const check = async () => {
      try {
        const login = await pixivLoginCheck();
        if (stopped) return;
        setWaiting(login.status === "waiting");
        if (login.status === "waiting") {
          timer = window.setTimeout(() => void check(), LOGIN_POLL_MS);
        } else if (login.status === "signedIn") {
          latest.current.onChange(login.info);
          latest.current.onNotice(t("已登录 {site}", { site: label }));
        }
      } catch (err) {
        if (stopped) return;
        setWaiting(false);
        setError(errorMessage(err));
      }
    };
    void check();
    return () => {
      stopped = true;
      window.clearTimeout(timer);
    };
  }, [round, signedIn, label]);

  const openLogin = async () => {
    setError(null);
    try {
      const result = await pixivLoginOpen();
      setWaiting(true);
      setRound((count) => count + 1);
      if (result.proxyFallback) {
        onNotice(t("登录窗口未能使用当前代理，已回退直连"));
      }
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const openBrowser = async () => {
    setError(null);
    try {
      await openUrl("https://www.pixiv.net/");
      onNotice(t("已在浏览器打开 Pixiv，请登录后粘贴 PHPSESSID"));
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const save = async (event: FormEvent) => {
    event.preventDefault();
    setBusy("save");
    setError(null);
    try {
      onChange(await accountSave(account.source, "", session));
      setSession("");
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
    <span className="badge warn">{t("需要重新登录")}</span>
  ) : (
    <span className="badge">{t("未登录")}</span>
  );

  return (
    <section className="set-card" aria-labelledby={`account-${account.source}`}>
      <div className="set-title">
        <h2 id={`account-${account.source}`}>{label}</h2>
        {status}
      </div>
      <p className="set-desc">{t("不登录也能搜全年龄作品、下载原图，但按 tag 搜最多翻 10 页。看 R-18 作品要先登录。")}</p>

      {signedIn ? (
        <div className="set-line">
          <div className="identity">
            <Icon name="user" size={15} />
            <b>{account.name}</b>
          </div>
          <div className="set-actions">
            <button type="button" className="btn ghost" onClick={() => void remove()} disabled={busy !== null}>
              {busy === "remove" ? t("正在退出…") : t("退出登录")}
            </button>
          </div>
        </div>
      ) : (
        <form className="form-row" onSubmit={(event) => void save(event)}>
          <label className="field">
            <span>PHPSESSID</span>
            <input
              className="field-input"
              type="password"
              value={session}
              onChange={(event) => setSession(event.target.value)}
              placeholder={t("粘贴 PHPSESSID")}
              autoComplete="off"
              spellCheck={false}
            />
          </label>
          <div className="set-actions">
            <button type="submit" className="btn" disabled={busy !== null}>
              <SwapLabel labels={[t("保存并验证"), t("正在验证…")]} active={busy === "save" ? 1 : 0} />
            </button>
            <button type="button" className="btn primary" onClick={() => void openBrowser()} disabled={busy !== null}>
              {t("在浏览器登录")}
            </button>
            <button type="button" className="btn ghost" onClick={() => void openLogin()} disabled={busy !== null}>
              <SwapLabel labels={[t("软件内登录"), t("等待登录…")]} active={waiting ? 1 : 0} />
            </button>
            {account.keyMissing && (
              <button type="button" className="btn ghost" onClick={() => void remove()} disabled={busy !== null}>
                {t("退出登录")}
              </button>
            )}
          </div>
        </form>
      )}

      {error && <p className="form-error">{error}</p>}
      {!signedIn && (
        <p className="form-hint">
          <span>
            {waiting
              ? t("在软件窗口里登录 Pixiv，登录好后这里会自动完成。")
              : t("推荐在浏览器里登录 Pixiv，再粘贴 Cookie 里的 PHPSESSID；软件内登录窗口会使用当前代理。")}
          </span>
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
          const key = `${account.source}-${account.name ?? ""}-${account.keyMissing}`;
          if (account.source === "pixiv") {
            return <PixivCard key={key} account={account} onChange={setInfo} onNotice={setNotice} />;
          }
          const site = SITES[account.source];
          return (
            site && <AccountCard key={key} site={site} account={account} onChange={setInfo} onNotice={setNotice} />
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
