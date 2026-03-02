// menu-demo.ts — Perry native menu bar demo
//
// Build (Linux/GTK4):
//   cargo run --release -- menu-demo.ts -o menu-demo && ./menu-demo

import {
  App,
  VStack,
  Text,
  Button,
  State,
  menuCreate,
  menuBarCreate,
  menuAddItem,
  menuAddSeparator,
  menuAddSubmenu,
  menuBarAddMenu,
  menuBarAttach,
} from "perry/ui";

// ── State ────────────────────────────────────────────────────────────────────
const status = State("Ready");

// ── Menu actions ─────────────────────────────────────────────────────────────
function onNew() {
  status.set("New file created");
}

function onOpen() {
  status.set("Open dialog would appear");
}

function onSave() {
  status.set("File saved");
}

function onQuit() {
  status.set("Quitting...");
}

function onCut() {
  status.set("Cut");
}

function onCopy() {
  status.set("Copied");
}

function onPaste() {
  status.set("Pasted");
}

function onAbout() {
  status.set("Perry Menu Demo v0.2.168");
}

// ── Build menus ───────────────────────────────────────────────────────────────

// File menu
const fileMenu = menuCreate();
menuAddItem(fileMenu, "New",  onNew,  "Cmd+N");
menuAddItem(fileMenu, "Open", onOpen, "Cmd+O");
menuAddItem(fileMenu, "Save", onSave, "Cmd+S");
menuAddSeparator(fileMenu);
menuAddItem(fileMenu, "Quit", onQuit, "Cmd+Q");

// Edit menu
const editMenu = menuCreate();
menuAddItem(editMenu, "Cut",   onCut,   "Cmd+X");
menuAddItem(editMenu, "Copy",  onCopy,  "Cmd+C");
menuAddItem(editMenu, "Paste", onPaste, "Cmd+V");

// View submenu nested inside Edit
const viewMenu = menuCreate();
menuAddItem(viewMenu, "Zoom In",  () => { status.set("Zoom in"); },  "Cmd+Shift+Equal");
menuAddItem(viewMenu, "Zoom Out", () => { status.set("Zoom out"); }, "Cmd+Minus");
menuAddSubmenu(editMenu, "View", viewMenu);

// Help menu
const helpMenu = menuCreate();
menuAddItem(helpMenu, "About", onAbout);

// Assemble the menu bar
const bar = menuBarCreate();
menuBarAddMenu(bar, "File", fileMenu);
menuBarAddMenu(bar, "Edit", editMenu);
menuBarAddMenu(bar, "Help", helpMenu);
menuBarAttach(bar);

// ── UI ────────────────────────────────────────────────────────────────────────
const label = status.text();

const btnClear = Button("Clear status");
btnClear.onClick(() => { status.set("Ready"); });

const root = VStack();
root.add(Text("Perry Menu Demo"));
root.add(label);
root.add(btnClear);

App("Menu Demo", root);
