import type { View } from "../components/Rail";
import type { SettingsSection } from "../features/settings/Settings";

/** 切换视图；去设置页时可以指定打开哪一栏。 */
export type Navigate = (view: View, section?: SettingsSection) => void;
