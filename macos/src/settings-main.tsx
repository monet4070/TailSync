import React from "react";
import ReactDOM from "react-dom/client";
import { Settings } from "./pages/Settings";
import "./index.css";
import "../../shared/art-direction.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Settings />
  </React.StrictMode>
);
