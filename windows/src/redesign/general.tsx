import { SectionShell, Row, RdToggle, ShortcutControl, type Chrome } from "./primitives";
import { MAC_SYNC_CAPS, MAC_HISTORY_CAPS, ZH, type MockState } from "./mock";

type Patch = (p: Partial<MockState>) => void;

export function GeneralSection({
  chrome,
  state,
  patch,
}: {
  chrome: Chrome;
  state: MockState;
  patch: Patch;
}) {
  const defaultSync = chrome === "mac" ? MAC_SYNC_CAPS : ["Win", "Alt", "/"];
  const defaultHistory = chrome === "mac" ? MAC_HISTORY_CAPS : ["Win", "Alt", ","];

  return (
    <SectionShell
      chrome={chrome}
      title={ZH.general}
      description={ZH.generalDescription}
    >
      <Row
        title={ZH.syncEnabled}
        desc={ZH.syncEnabledDescription}
        control={
          <RdToggle
            checked={state.syncEnabled}
            onChange={(v) => patch({ syncEnabled: v })}
            label={ZH.syncEnabled}
          />
        }
        onClick={() => patch({ syncEnabled: !state.syncEnabled })}
      />

      <Row
        title={ZH.launchAtLogin}
        desc={ZH.launchAtLoginDescription}
        control={
          <RdToggle
            checked={state.launchAtLogin}
            onChange={(v) => patch({ launchAtLogin: v })}
            label={ZH.launchAtLogin}
          />
        }
        onClick={() => patch({ launchAtLogin: !state.launchAtLogin })}
      />

      {/* 快捷键：凸显立体实体键帽，解决 Mac 版快捷键不明显、空白过大的问题 */}
      <Row
        title={ZH.syncShortcut}
        desc={ZH.syncShortcutDescription}
        control={
          <ShortcutControl
            chrome={chrome}
            caps={state.syncShortcut}
            defaultCaps={defaultSync}
            onChange={(caps) => patch({ syncShortcut: caps })}
            recordLabel={ZH.syncShortcut}
          />
        }
      />

      <Row
        title={ZH.historyShortcut}
        desc={ZH.historyShortcutDescription}
        control={
          <ShortcutControl
            chrome={chrome}
            caps={state.historyShortcut}
            defaultCaps={defaultHistory}
            onChange={(caps) => patch({ historyShortcut: caps })}
            recordLabel={ZH.historyShortcut}
          />
        }
      />

      <Row
        title={ZH.notifications}
        desc={ZH.notificationsDescription}
        control={
          <RdToggle
            checked={state.notifications}
            onChange={(v) => patch({ notifications: v })}
            label={ZH.notifications}
          />
        }
        onClick={() => patch({ notifications: !state.notifications })}
      />

      <Row
        title={ZH.progressBar}
        desc={ZH.progressDescription}
        control={
          <RdToggle
            checked={state.progressBar}
            onChange={(v) => patch({ progressBar: v })}
            label={ZH.progressBar}
          />
        }
        onClick={() => patch({ progressBar: !state.progressBar })}
      />
    </SectionShell>
  );
}
