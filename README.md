# diskscope

**A fast, safety-first disk space analyzer and cleanup tool for Windows, written in Rust.**

diskscope shows where your disk space went: a folder tree, an extension table and a treemap. It also suggests what you can safely clean up, and lets you delete only after you review and confirm.

*Inspired by WizTree. diskscope is an independent open-source project and is not affiliated with or endorsed by WizTree or its authors.*

![diskscope: folder tree, extension table, treemap and the delete bar](docs/images/tree.png)

## The problem

- **Dev machines fill up.** `node_modules`, the pnpm / npm / Yarn caches, Unity `Library` folders, Rust `target` folders, Gradle caches and old git worktrees quietly eat tens to hundreds of GB.
- **Finding the culprits is slow.** Explorer's *Properties* crawls, and classic folder walkers take minutes: diskscope's own walker needed **13.6 minutes** for a cold scan of a 3-million-item C: drive.
- **It is hard to know what is safe to delete.** Is this folder a cache that rebuilds itself, or work you still need?
- **One wrong Shift+Del is gone forever.** Most tools delete immediately.
- **There is no record of what you deleted** last week, or why the disk is suddenly full again.

## What diskscope does about it

- **Native Rust app: a single 8 MB exe.** No runtime, no installer, memory-safe code.
- **Fast NTFS scan (run as administrator).**
  - It reads the Master File Table directly, like WizTree: a 3.0M-item C: in **10.7 s** and a 6.7M-item D: in **9.5 s** on the test PC.
  - Results are within **0.4-0.6 %** of the volume's used space.
- **True space used.** Hard links (pnpm, conda) are counted once, so the numbers add up to what the disk really holds.
- **Lower memory** than the tool it was compared with (see the table below; measured numbers only).
- **Live cleanup suggestions while scanning.**
  - Results are ranked by reclaimable size × confidence: *safe cache*, *likely safe* or *check first*.
  - Each suggestion has a one-line reason and its last-change date.
  - Rules cover npm/pnpm/Yarn caches, `node_modules` of stale projects, Rust `target`, Unity `Library`/`Bee`/`Temp`, Gradle, `dist`, `__pycache__`, `.venv`, the user temp folder, older app versions in AppData, and git worktrees of finished AI-agent sessions (via the `ao` CLI, if installed).
- **Review-first deletion.**
  - Nothing is ever ticked for you.
  - **Del** moves to the Recycle Bin. **Shift+Del** deletes permanently and requires typing `DELETE`.
  - Both show every path and the total first.
- **90-day delete History.** Searchable, exportable to CSV, and it reminds you what is still restorable from the Recycle Bin.
- **Read-only scanning.** The volume is opened for reading only. Without admin rights, diskscope falls back to a parallel folder walker.

## Measured numbers

Everything below was measured on one Windows 11 PC with an NVMe SSD (C:) on 2026-11-22. No number is estimated or taken from elsewhere.

### diskscope vs WizTree 4.31: same C: drive (3.0M items), elevated, warm (2nd of 2 runs)

| | diskscope 0.1.0 (`diskscope-cli scan C:\ --mft`) | WizTree 4.31 (`WizTree64.exe C: /export=… /exportfolders=1 /exportfiles=0`) |
|---|---|---|
| Wall time | **5.5 s** (scan + report) | 15.2 s (scan + writing an 82 MB folder CSV) |
| Peak working set | **494 MB** | 1,440 MB |

Both runs were measured by the same script, which polled each process's OS-reported peak working set. WizTree's wall time includes its CSV export, so treat the time column as indicative and the memory column as the main comparison.

### diskscope on its own

| Target | Fast NTFS (MFT) scan | Folder walker (no admin) | Accuracy |
|---|---|---|---|
| C:, 3.0M items | 10.7 s (first elevated run), 5.5 s warm | 13.6 min cold, ~2 min warm | 386.3 GB allocated vs 387.9 GB volume used (−0.4 %) |
| D:, 6.7M items | 9.5 s; peak 940 MB | 4.5 min cold, 2.4 min warm; window ~450 MB | 494.4 GB vs 497.4 GB used (−0.6 %) |
| A 45 GB dev-tools folder | 5.2 s | 0.7 s warm | identical: 231,791 files, 44.8 GB |

The D: peak (940 MB) is the peak private memory of the CLI process. On folders with many hard links, the MFT scan reports less than the walker (or WizTree), because each file is counted once. Folders without hard links match to the byte.

## Download

Get `diskscope.exe` from the [latest release](https://github.com/nqvinh/diskscope/releases/latest). The SHA-256 checksum is in the release notes. It runs on Windows 10/11 x64; just start the exe.

## Quick start

1. Start `diskscope.exe`, pick a drive (or type or browse to a folder) and press **Scan**.
2. Watch the **best cleanup suggestions** appear while the scan runs. Press **Add** on the rows you want, or **Add all safe**.
3. Check the **Review** tab, or select an item in the tree.
4. Use the big buttons at the bottom:
   - **Move to Recycle Bin (Del)**: confirm in the dialog and you are done.
   - **Delete permanently (Shift+Del)**: confirm, then type `DELETE`.
5. Look back any time in **History**.

![Ranked cleanup suggestions with confidence levels](docs/images/candidates.png)

Command line: `diskscope.exe <folder>` opens and scans a folder. `diskscope.exe <folder> --select <item>` also selects an item, and `--tab candidates` opens a tab.

## Run as administrator for the fast scan

Reading the MFT needs administrator rights. Without them, diskscope uses its parallel folder walker and shows **Restart as administrator** in the toolbar: Windows asks once and the fast scan is used from then on. The exe never demands admin rights on its own.

## Safety model

- **Suggestions only.** Nothing is pre-selected, and "Add all safe" only fills the Review list.
- **Confirmation for every delete.** The dialog lists every path and the total, and you must tick *I reviewed every path in this list*. Permanent delete adds a second dialog where you type `DELETE`. In code, the delete function only accepts a plan built by these dialogs.
- **Recycle Bin by default.** The dialog reminds you that the bin frees no space until it is emptied. Items too big for the bin are skipped, never deleted permanently.
- **Hard blocklist**, checked when you add an item and again right before deleting it:
  - drive roots
  - `Windows`, `Program Files*` and `ProgramData\Microsoft`
  - pagefile / hiberfil / swapfile and NTFS metafiles
  - `System Volume Information` and `$Recycle.Bin`
  - user profile roots and their main folders, and the AppData roots
- **Skipped and reported**: files in use, access-denied items, and items that changed since the scan.
- **Logged.** Every action goes to `%LOCALAPPDATA%\diskscope` (History plus an action log).
- **Read-only scanning.** The volume is opened with read access only. The CLI has no delete command at all.

## How it works

**Fast path (NTFS, elevated):**
1. diskscope opens `\\.\C:` for reading and asks for the volume layout (`FSCTL_GET_NTFS_VOLUME_DATA`, a read-only query).
2. It maps `$MFT`'s own data runs, including attribute-list fragments on a heavily fragmented MFT.
3. It reads `$MFT` sequentially in 8 MiB chunks while rayon parses the FILE records in parallel. For each record it applies the update-sequence fixups, merges base and extension records, and reads the `$STANDARD_INFORMATION` times, the Win32 name (skipping DOS short names) and the unnamed `$DATA` size.
4. It builds the folder tree from the parent references.

**Fallback:** a 32-thread directory walker that never follows junctions, symlinks or mount points.

**Both paths** share one tree builder. In busy folders it groups many small files into one entry per extension, with exact totals, to keep memory low.

## Build from source

```powershell
git clone https://github.com/nqvinh/diskscope
cd diskscope
cargo build --release        # Rust 1.95 is pinned in rust-toolchain.toml
# target\release\diskscope.exe (window) and target\release\diskscope-cli.exe (read-only report)
```

## Tests

```powershell
cargo test --release
```

44 tests cover:
- the MFT record parser (on synthetic FILE records) and the tree builder
- aggregation, the treemap layout and the blocklist
- suggestion rules and ranking
- the confirmation gate, Del / Shift+Del routing, and deleting only the listed items
- small-file grouping, and junctions never being followed (on a real temporary folder)
- history append and the 90-day prune

## License

MIT, see [LICENSE](LICENSE).
