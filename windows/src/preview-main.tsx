import React from "react";
import ReactDOM from "react-dom/client";
import { Preview } from "./pages/Preview";
import "./index.css";
import "../../shared/art-direction.css";
import "./styles/preview.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Preview />
  </React.StrictMode>,
);
