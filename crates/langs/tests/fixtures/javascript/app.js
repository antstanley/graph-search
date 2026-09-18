const { store } = require("./store");
import { ui } from "./ui.js";

export function boot(el) {
  store.subscribe(ui.draw);
}

class Panel {
  constructor(root) {
    this.root = root;
  }
  open() {
    boot(this.root);
  }
}

const VERSION = "2";
