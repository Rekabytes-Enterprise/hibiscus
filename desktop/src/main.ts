import { mount } from "svelte";
import App from "./App.svelte";
import "./app.css";

// macOS draws the window buttons over the content and frosts the sidebar.
document.documentElement.classList.toggle("mac", navigator.userAgent.includes("Mac"));

mount(App, { target: document.getElementById("app")! });
