import { mount } from "svelte";
import App from "./App.svelte";
import ComposeWindow from "./lib/ComposeWindow.svelte";
import "./app.css";

// A Compose window of its own loads the same page with ?compose=<window label>.
const composeLabel = new URLSearchParams(location.search).get("compose");
mount(composeLabel ? ComposeWindow : App, { target: document.getElementById("app"), props: composeLabel ? { label: composeLabel } : {} });
