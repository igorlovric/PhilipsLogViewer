# Philips Log Viewer

Desktop viewer for Philips system logs (`.xml`, zipped `.xml`, and CT log databases `.mdb`) – Rust + egui, a single `.exe`, no installation.

Version history: see `CHANGELOG.md`, or click the program name / version in the top left corner of the program.

**Download**: the ready-made exe of every version is under [Releases](https://github.com/igorlovric/PhilipsLogViewer/releases).

To release a new version: raise `version` in `Cargo.toml` and add an entry at the top of `CHANGELOG` in `src/version.rs`
(and the same text in `CHANGELOG.md`). On the first start of a new version the program shows "What's new".
When the change reaches the `main` branch, GitHub Actions (`.github/workflows/release.yml`) builds the exe on Windows
and publishes the release `v<version>` with the notes from `CHANGELOG.md` – nothing has to be built or uploaded by hand.

## Running

`philips-log-viewer.exe` – double-click it, then **☰ → Open…** (menu in the top left corner, or Ctrl+O), or drag an `.xml`/`.zip`/`.mdb` into the window.

The **☰ menu** contains: Open, Files in ZIP, System, Charts, Codes, Settings, Merge patterns, About.
You can also right-click a log → *Open with* → choose the exe (the file is passed as an argument).

## Building on Windows (one-time setup)

1. **Visual Studio Build Tools** (the C++ compiler and linker Rust uses). In PowerShell:
   ```powershell
   winget install Microsoft.VisualStudio.2022.BuildTools --override "--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --passive"
   ```
   (or manually from https://visualstudio.microsoft.com/downloads/ → *Build Tools for Visual Studio* → tick **Desktop development with C++**)
2. **Rust**:
   ```powershell
   winget install Rustlang.Rustup
   ```
   Close and reopen the terminal, then check: `cargo --version`
3. **Build** – in the project folder:
   ```powershell
   cargo build --release
   ```
   The program is at `target\release\philips-log-viewer.exe`.
   The first build takes a few minutes (downloads and compiles libraries); later builds are fast.
   To try changes quickly: `cargo run --release -- "path\to\log.zip"`

## Features

- **Table** without paging – all rows are loaded and only the visible part is drawn, so scrolling is instant even with 100,000+ rows.
- **Row colors**: Error red · Fatal purple · Warning yellow · Information light green · UserMessage dark green · Command (description starts with "Command") no color.
  All of them, and the color of selected rows, can be changed in **☰ → Settings → Colors**. Categories found in a log that are not in the list yet are added there automatically, starting with the color of the default they belong to.
- **Filters in every column header**: text (`a|b` = or, `!x` = not), and `⏷` opens a list of values with counts (tick boxes).
  Categories at the top (Information/Warning/Error/Fatal): click = show/hide, right-click = only this one.
  Right-click a row → "Filter by value".
- **Search** (Ctrl+F): searches description, Event ID, Additional Info, unit, process. Enter/F3 next, Shift+F3 previous, "matches only" filters the table.
- **Timeline**: system mode band (green Normal, blue Startup, gray Shutdown, orange WarmRestart, purple FieldService), event density, warning/error/fatal markers and recognized patterns, frame showing the rows currently visible.
  Click = jump to that time (or to the error under the cursor) · drag = time range filter · drag inside the selected range = move it left/right · drag its edge = make it wider/narrower · wheel = zoom · right-drag = pan · double-click = whole log.
- **Details panel**: dock it left, bottom or right (◀ ▼ ▶), or put it in a separate window (🗖) that can be moved anywhere, also outside the main window. It keeps the size you drag it to.
  **Formatted / Raw**: *Formatted* shows `\n` as real line breaks, `{key = value}` / `[key=value]` groups and `a: b; c: d` lists as tables, `*values*` highlighted, and analyzes .NET and Java stack traces (root cause, where it was thrown, first application frame, exception chain, a short explanation of known errors). *Raw* shows the text exactly as in the log.
  **POST reports** (Power-On Self Test, texts with `POSTItem:`) are shown as a test report: result, failed tests first, all test items with ✔ / ✖, measured values with their limits, and the involved FRUs.
- **Files in the ZIP** (☰ → Files in ZIP): every file of the archive grouped by folder, with a short description of known files.
  Click a name to view it (search, errors/warnings highlighted, UTF-8 and UTF-16), 📊 opens a text or trace log as a table with all features (↩ Main log goes back),
  ↗ opens a file with its Windows program (e.g. `.evtx` in Event Viewer), 💾 saves a copy.
  Recognized table formats: Platform trace files (`trace-*.txt`), tab-separated logs with a header (e.g. CAN trace), and text logs whose lines start with a timestamp
  (`31.08.2026 17:51:24.634`, `2026-08-31 17:51:24`, `20260831-130722.289`, `08/19/26 10:47:01`, `Aug 31 2026:17:51:01`, `[2026-08-31 17:48:20]`, or time only).
- **CT log databases** (`Log_Database_Export_*.mdb`): open them like any log (also from inside a ZIP). Event type "System Error" is shown as Error, the error code becomes the Event ID, and the key = value details of every event are shown as a table.
- **Code dictionary** (☰ → Codes): click an error row → "📘 Describe code…" (or right-click a row) and write what the code means. The description is shown next to the code in the table and in the details, search finds it, and it is stored in the global config, so colleagues get it too. The Codes window also lists the error / warning codes of the current log that have no description yet.
- **Charts** (☰ → Charts): every numeric value found in the event details (e.g. CT HeatCapacity: HeatStorage, HV, tube temperatures; Scan Parameter: kV, mA; Scan Finished: CTDIvol, DLP; DMS Temperature; Azurion DVI grabber temperatures; generator load level) can be shown over time. All charts share the time axis (wheel = zoom, drag = pan, double-click = reset); clicking a point shows that event in the table. The chosen values are remembered for the next log.
- **System info** (☰ → System): product, versions, serial number, site, installed software, hardware and licenses from the log header.
- Only XML files that contain `<Event>` records are loaded into the main table; the other XML files in the ZIP are available under ☰ → Files in ZIP.
- **Columns** (☰ → Settings, or right-click the table header): each kind of log (Azurion, CombiDiagnost, CT database, trace / text logs…) can have its own columns; kinds without their own choice use the default set. Columns that are empty in the current log are hidden automatically.
- **Bookmarks**: double-click a row or press Ctrl+B. Write a comment for the bookmark in the bottom panel. **📋 List & notes** shows all bookmarks with comments and notes for the whole log.
- **Shortcuts**: F8 / Shift+F8 next/previous warning-error · Ctrl+B bookmark · F2 next bookmark · Ctrl+C copies selected rows (for Excel) · Ctrl+A select all · arrows/PageUp/PageDown.
- **Merge patterns** (☰ → Merge patterns…): choose a colleague's `philips-log-viewer.config.json` (or an exported pattern list). Every pattern is shown as *new*, *already in your library* or *same name but different steps*; you choose Add / Skip / Keep both / Replace mine. Your own patterns are never removed.
- **Patterns**: select rows (Ctrl+click / Shift+click) → **➕ Add pattern** (or Ctrl+P, or right-click).
  Enter a name, description, comment, color and tolerances. Every time a log is loaded the patterns are searched automatically;
  the **Patterns** button turns orange and shows how many were recognized. The window lists each occurrence with % similarity and a "Show" button.

## How pattern recognition works

- Each pattern step is compared by **Event ID + description** (numbers in the description are ignored); Event ID only or description only are also available.
- Gaps between rows may deviate by the **tolerance** (default ±50 % of the gap, at least ±2 s).
- The order may differ slightly – swapped rows lower the similarity, and so do missing steps.
- Occurrences above the **similarity threshold** (default 75 %) are listed.

## What is saved where (JSON, readable in any editor)

**Next to each log** – same name, extension `.plv.json`, in the same folder:
`CAT_CV_..._09_19_34.631.zip` → `CAT_CV_..._09_19_34.631.plv.json`
Contains the notes for the log, bookmarks with comments and the view state (filters, search, sorting, time range).
For a log opened from inside the ZIP (📊) the file is called `<zip name>__<file name>.plv.json`.
It is saved automatically a second after each change and restored when you open that log again.

**Global** – `philips-log-viewer.config.json` next to the exe: all patterns, the code dictionary and settings (visible columns, row colors per category, last folder, details panel position and size, Formatted/Raw, chosen chart values).
To give the program to a colleague, copy the exe and this file.
If the exe folder is not writable (e.g. Program Files), the file goes to `%APPDATA%\PhilipsLogViewer\`.

## Code structure

| File | Contents |
|---|---|
| `src/main.rs` | startup, fonts (Segoe UI if present), icon |
| `src/config.rs` | global config and the `.plv.json` file next to the log |
| `build.rs`, `assets/` | icon embedded into the exe |
| `src/parser.rs` | fast streaming XML/ZIP reading on a background thread |
| `src/model.rs` | event data, categories, time formatting |
| `src/app.rs` | the whole UI: table, filters, search, details, pattern windows |
| `src/files_ui.rs` | file list, file viewer, system info window |
| `src/files.rs` | decoding, file descriptions, text / trace logs → table |
| `src/formatter.rs` | Formatted view: line breaks, key = value tables, stack trace analysis |
| `src/post.rs` | Power-On Self Test reports |
| `src/xmltree.rs` | XML tree for the system info |
| `src/mdb.rs` | CT log databases (.mdb) via the `jetdb` crate |
| `src/codes_ui.rs` | code dictionary |
| `src/charts.rs` | charts of numeric values (`egui_plot`) |
| `src/version.rs` | version number and change history |
| `src/timeline.rs` | timeline |
| `src/patterns.rs` | pattern definition and recognition |
| `src/theme.rs` | colors |
| `src/colors_ui.rs` | Settings → Colors (row color per event category, selected row) |
| `src/menu_ui.rs` | ☰ menu, Settings → Table columns, merging patterns |
