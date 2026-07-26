import { useTheme } from "./hooks/useTheme";

/**
 * Main App component.
 * This window is hidden by Tauri at startup - the app runs from the system tray.
 */
function App() {
  const { theme } = useTheme();

  return (
    <div className={`app ${theme}`}>
      <div style={{ display: "none" }}>{/* Tray-only app */}</div>
    </div>
  );
}

export default App;
