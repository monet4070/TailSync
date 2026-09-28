import React from "react";
import ReactDOM from "react-dom/client";
import { Connections } from "./pages/Connections";
import "./index.css";
import "../../shared/art-direction.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Connections />
  </React.StrictMode>
);
