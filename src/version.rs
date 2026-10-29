//! Program version and change history (shown in About / What's new).

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const BUILD_DATE: &str = env!("PLV_BUILD_DATE");

pub struct Release {
    pub version: &'static str,
    pub date: &'static str,
    pub changes: &'static [&'static str],
}

/// Newest first. Add an entry here (and bump the version in Cargo.toml) for every release.
pub const CHANGELOG: &[Release] = &[
    Release {
        version: "1.9.1",
        date: "2026-10-29",
        changes: &[
            "Information window: product names for the Philips ultrasound product numbers (Affiniti, EPIQ, ClearVue, Sparq, Lumify, InnoSight…); numbers not confirmed on philips.com are shown as “probably …”",
        ],
    },
    Release {
        version: "1.9.0",
        date: "2026-10-29",
        changes: &[
            "Encrypted Philips files (e.g. ultrasound transducer tests, .enc) open an information window instead of an error: content, modality, product, serial number, software version, date and time from the file name, and a note that the file is encrypted and needs the Philips service tools",
            "Archives without an event log (e.g. ultrasound utilization reports) open an information window: product, model, serial number, software, institution from MedicalSystem.xml, the hardware and software parts, and buttons that open the PDF reports",
            "Files in ZIP: an encrypted file shows the same information window when clicked",
        ],
    },
    Release {
        version: "1.8.1",
        date: "2026-10-29",
        changes: &[
            "Fix: a small error in PhilipsLogViewer.config (e.g. a setting written twice) no longer resets all settings – the file is repaired, invalid values fall back to their defaults one by one, and nothing else is lost",
            "When the settings file had to be repaired, the program says what happened and keeps the original as PhilipsLogViewer.config.bak-<date>",
        ],
    },
    Release {
        version: "1.8.0",
        date: "2026-10-28",
        changes: &[
            "Shared patterns: share your patterns with colleagues and add theirs, right from the program – no account, no browser (☰ → Shared patterns)",
            "Only patterns you tick “Share” are sent; later changes to them are sent automatically, and colleagues get them at their next start",
            "Editing a colleague's pattern makes your own copy; the shared original stays unchanged",
            "When an author withdraws a pattern, you are asked whether to delete your copy too",
            "Every pattern now has a unique id and a checksum (to recognize changes); existing patterns get one automatically",
            "Your name for shared patterns: taken from the Windows user name, confirmed at the first start, changeable in Settings → Sharing",
            "Server address and key are built in and can be overridden in PhilipsLogViewer.config (share_server, share_api_key) for testing",
        ],
    },
    Release {
        version: "1.7.1",
        date: "2026-10-28",
        changes: &[
            "The settings file is now called PhilipsLogViewer.config (still JSON); a philips-log-viewer.config.json from an earlier version is renamed automatically at startup, nothing is lost",
            "Merge patterns accepts both the new .config and the old .json files",
        ],
    },
    Release {
        version: "1.7.0",
        date: "2026-10-28",
        changes: &[
            "The program file is now always called PhilipsLogViewer.exe (no version in the name)",
            "Update check: at startup the program asks GitHub whether there is a new version; if so, a button appears in the top right corner with what's new, Update now / Later / Skip this version",
            "Update now downloads the new version, checks it, replaces the program and starts it again with the same log – settings, patterns and notes stay as they are",
            "Settings → Updates: switch the check at startup on / off, or let updates install automatically (downloaded in the background, put in place when the program closes)",
            "☰ → Check for updates… to check at any time",
        ],
    },
    Release {
        version: "1.6.0",
        date: "2026-10-28",
        changes: &[
            "Settings → Colors: choose the row color of every event category (Information, Warning, Error, Fatal, UserMessage, Command…) or leave a category without color; the defaults are the colors used so far",
            "Categories found in a log that are not in the list yet are added automatically and start with the color of the default they belong to; you can also add a category by name",
            "Settings → Colors: choose the color of selected rows",
            "The colors are stored in the global config, so a colleague who gets the file gets them too",
        ],
    },
    Release {
        version: "1.5.0",
        date: "2026-10-28",
        changes: &[
            "☰ menu in the top left corner: Open, Files in ZIP, System, Charts, Codes, Settings, Merge patterns, About (replaces the separate buttons)",
            "Settings: choose the table columns separately for each kind of log (Azurion, CombiDiagnost, CT database, trace / text logs…) or use the default set",
            "Right-click the table header to choose the columns of the current log",
            "Merge patterns: load a colleague's config / pattern file and merge his patterns with yours (new, already present, or same name with different steps – Add / Skip / Keep both / Replace mine)",
            "Timeline: drag the selected time range to move it left / right, drag its edges to resize it",
            "Buttons renamed: “Add pattern”, “Matches only”",
        ],
    },
    Release {
        version: "1.4.0",
        date: "2026-10-26",
        changes: &[
            "Power-On Self Test reports (POSTItem …) are shown as a test report: result, failed tests with their details first, all test items with pass/fail marks, measured values with their limits as a small bar, and the involved FRUs with part codes",
            "Works for the different POST texts (Done/Passed, Done/Failed, Skipped, with or without [Description] / [Details]); other texts are formatted as before",
        ],
    },
    Release {
        version: "1.3.1",
        date: "2026-10-26",
        changes: &[
            "Formatted view: lists inside [key=value] groups are split into table rows – \"a=1; b=2\", \"a=1 b=2\" and \"name(a=1; b=2)\" (e.g. [Exception Info=[detail=eds(DRV=3; …), MMS=2; …]], [Configuration=A = 1; B = 2])",
            "A table row that groups sub-values also shows its own value (e.g. detail = eds)",
        ],
    },
    Release {
        version: "1.3.0",
        date: "2026-10-26",
        changes: &[
            "CT event logs from Access databases (.mdb / .accdb, e.g. Log_Database_Export_*.mdb) open directly – also from inside a ZIP",
            "Code dictionary (📘 Codes): describe an error code / Event ID once; the description is shown in the table, the details and found by search in every log, and stored in the global config",
            "List of error / warning codes of the current log that have no description yet",
            "Charts (📈): numeric values from the event details over time (tube heat and temperatures, kV, mA, DLP, gantry temperature, detector temperatures…); shared time axis, click a point to show the event",
            "The chosen chart values are remembered for the next log",
            "Header shows only the product information that the log contains",
        ],
    },
    Release {
        version: "1.2.0",
        date: "2026-10-26",
        changes: &[
            "ZIP with several XML files: only XML files with <Event> records go to the table, the others are listed under Files",
            "Details panel keeps its size; dock it left, bottom or right, or open it in a separate window",
            "Formatted / Raw view: line breaks, key = value tables, highlighted *values*",
            "Exception analysis for .NET and Java stack traces (root cause, where thrown, exception chain, explanation of known errors)",
            "Files in the ZIP: view any file, open text and trace logs as a table, open externally, save a copy",
            "System info: product, versions, site, software, hardware and licenses",
            "New columns Component and Package; columns empty in the current log are hidden",
            "Text filter =x for an exact match",
        ],
    },
    Release {
        version: "1.1.1",
        date: "2026-10-25",
        changes: &[
            "Clicking on text in the table always selects the row",
            "All texts in English; English number and date formats",
        ],
    },
    Release {
        version: "1.1.0",
        date: "2026-10-25",
        changes: &[
            "Bookmarks with comments and notes per log, saved next to the log as <name>.plv.json",
            "Filters, search and sorting are remembered per log",
            "Patterns and settings in philips-log-viewer.config.json next to the program (easy to copy to a colleague)",
            "New name Philips Log Viewer, program icon, light theme only",
        ],
    },
    Release {
        version: "1.0.0",
        date: "2026-10-25",
        changes: &[
            "Fast table for XML / zipped XML logs without paging",
            "Column filters, category filters, search",
            "Row colors by event category",
            "Timeline with system modes, warnings and errors",
            "Pattern definition and automatic recognition",
        ],
    },
];
