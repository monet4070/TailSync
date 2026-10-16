import React from "react";
import ReactDOM from "react-dom/client";
import { RedesignApp } from "./RedesignApp";
import "../index.css";
import "../../../shared/art-direction.css";
import "./redesign.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <RedesignApp />
  </React.StrictMode>
);
