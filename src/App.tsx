import { useState, type ReactNode } from "react";
import { MotionConfig, motion } from "motion/react";

import { Rail, type View } from "./components/Rail";
import { Discover } from "./features/discover/Discover";
import { Downloads } from "./features/downloads/Downloads";
import { DownloadsProvider, useDownloads } from "./features/downloads/DownloadsProvider";
import { Library } from "./features/library/Library";
import { Settings } from "./features/settings/Settings";
import { VIEW_VARIANTS } from "./lib/motion";
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
  const { activeCount } = useDownloads();
  return (
    <div className="app">
      <Rail view={view} onChange={setView} badges={{ downloads: activeCount }} />
      <main className="app-main">
        <ViewPane active={view === "discover"}>
          <Discover active={view === "discover"} onNavigate={setView} />
        </ViewPane>
        <ViewPane active={view === "library"}>
          <Library active={view === "library"} onNavigate={setView} />
        </ViewPane>
        <ViewPane active={view === "downloads"}>
          <Downloads onNavigate={setView} />
        </ViewPane>
        <ViewPane active={view === "settings"}>
          <Settings />
        </ViewPane>
      </main>
    </div>
  );
}

export default function App() {
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
