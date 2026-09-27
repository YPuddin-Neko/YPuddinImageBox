import { useCallback, useEffect, useState, type ReactNode } from "react";
import { MotionConfig, motion } from "motion/react";

import { Rail, type View } from "./components/Rail";
import { WindowControls } from "./components/WindowControls";
import { Discover } from "./features/discover/Discover";
import { Downloads } from "./features/downloads/Downloads";
import { useDownloads } from "./features/downloads/context";
import { DownloadsProvider } from "./features/downloads/DownloadsProvider";
import { Library } from "./features/library/Library";
import { Settings, type SettingsSection } from "./features/settings/Settings";
import { Subscriptions } from "./features/subscriptions/Subscriptions";
import { dialogOpen, hasMod } from "./lib/hotkeys";
import { useLanguage } from "./lib/i18n";
import { VIEW_VARIANTS } from "./lib/motion";
import type { Navigate } from "./lib/nav";
import { isWindows } from "./lib/platform";
import { ThemeProvider } from "./theme/ThemeProvider";

function ViewPane({ active, children }: { active: boolean; children: ReactNode }) {
  return (
    <motion.section
      className="view"
      initial={false}
      animate={active ? "shown" : "hidden"}
      variants={VIEW_VARIANTS}
      inert={!active}
      aria-hidden={!active}
    >
      {children}
    </motion.section>
  );
}

/** 各视图一直挂着，切换时只做淡入淡出，搜索结果、滚动位置都保留。 */
function Shell() {
  const [view, setView] = useState<View>("discover");
  const [section, setSection] = useState<SettingsSection>("general");
  const { activeCount } = useDownloads();
  const navigate = useCallback<Navigate>((next, target) => {
    if (target) setSection(target);
    setView(next);
  }, []);

  // 全局快捷键：⌘/Ctrl + 1～4 切换页面，⌘/Ctrl + , 打开设置，⌘/Ctrl + F 跳到搜索框。输入框里也能用。
  useEffect(() => {
    const searchInput = () => document.querySelector<HTMLInputElement>('.view[aria-hidden="false"] .search-input');
    const onKey = (event: KeyboardEvent) => {
      if (!hasMod(event) || event.shiftKey || event.isComposing || dialogOpen()) return;
      const pages: Record<string, View> = { "1": "discover", "2": "library", "3": "subscriptions", "4": "downloads", ",": "settings" };
      const page = pages[event.key];
      if (page) {
        event.preventDefault();
        setView(page);
      } else if (event.key.toLowerCase() === "f") {
        event.preventDefault();
        const input = searchInput();
        if (input) {
          input.focus();
          input.select();
        } else {
          // 当前页没有搜索框（订阅、下载、设置）：先回到发现页，等它显示出来再聚焦。
          setView("discover");
          window.setTimeout(() => {
            searchInput()?.focus();
            searchInput()?.select();
          }, 60);
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  return (
    <div className="app">
      {isWindows && <WindowControls />}
      <Rail view={view} onChange={setView} badges={{ downloads: activeCount }} />
      <main className="app-main">
        <ViewPane active={view === "discover"}>
          <Discover active={view === "discover"} onNavigate={navigate} />
        </ViewPane>
        <ViewPane active={view === "library"}>
          <Library active={view === "library"} onNavigate={navigate} />
        </ViewPane>
        <ViewPane active={view === "subscriptions"}>
          <Subscriptions onNavigate={navigate} />
        </ViewPane>
        <ViewPane active={view === "downloads"}>
          <Downloads onNavigate={navigate} />
        </ViewPane>
        <ViewPane active={view === "settings"}>
          <Settings section={section} onSectionChange={setSection} />
        </ViewPane>
      </main>
    </div>
  );
}

export default function App() {
  // 切换语言时从这里整个重新渲染。
  useLanguage();
  return (
    <ThemeProvider>
      {/* 系统开启「减弱动态效果」时，Motion 只保留淡入淡出，不做位移。 */}
      <MotionConfig reducedMotion="user">
        <DownloadsProvider>
          <Shell />
        </DownloadsProvider>
      </MotionConfig>
    </ThemeProvider>
  );
}
