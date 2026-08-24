import React from "react";
import ReactDOM from "react-dom/client";

import "@fontsource/eb-garamond/400.css";
import "@fontsource/eb-garamond/500.css";
import "@fontsource/figtree/400.css";
import "@fontsource/figtree/500.css";
import "@fontsource/figtree/600.css";
import "./styles.css";

import App from "./App";
import { resolveLang, setLang } from "./i18n";

// Language is picked once from the system locale before the first render, so no
// component ever paints an English string and then swaps it.
setLang(resolveLang(localStorage.getItem("wortlaut.lang") ?? undefined));

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
