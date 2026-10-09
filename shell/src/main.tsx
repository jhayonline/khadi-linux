import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import Greeter from "./Greeter";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "./index.css";

// One bundle, two surfaces, told apart by the WINDOW LABEL — which Tauri
// injects before this script runs, so the decision is synchronous and the
// login screen never shows a frame of the desktop first. The first attempt
// keyed off `location.hash`, nothing ever set it, and the greeter came up as
// the desktop complaining that `pty_spawn` was not a registered command.
// (It is not, in greeter mode, and that is the point — see greet.rs.)
const greeter = getCurrentWindow().label === "greeter";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>{greeter ? <Greeter /> : <App />}</React.StrictMode>,
);
