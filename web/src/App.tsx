import type { Component } from "solid-js";
import { HashRouter, Route } from "@solidjs/router";
import { LoadCast } from "./views/LoadCast";
import { Editor } from "./views/Editor";

/**
 * Application shell + routing.
 *
 * The editor is the landing route (`/`) — the server already loaded a cast, so
 * the cutting-room opens directly (matching `prototype/ux/index.html`, which
 * has no load screen). A secondary `/load` route keeps the drop-a-file
 * affordance for previewing an arbitrary client-selected cast in-memory.
 * A HashRouter keeps client routes working under the server's `spa_fallback`
 * (T07) without per-route server rewrites.
 */
export const App: Component = () => (
  <HashRouter>
    <Route path="/" component={Editor} />
    <Route path="/load" component={LoadCast} />
    <Route path="*" component={Editor} />
  </HashRouter>
);