import React from "react";
import ReactDOM from "react-dom/client";
import { History } from "./pages/History";
import "./index.css";
import "../../shared/art-direction.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <History />
  </React.StrictMode>
);
