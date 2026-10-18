# SPYDER Bone — User Guide

SPYDER Bone scores ASD LabSpec 4 near-infrared scans of bone, one scan at a time. It tells you how promising each scan
looks for destructive sampling (radiocarbon dating, ZooMS, stable isotopes) before you commit powder to it. It runs on
Windows and macOS. It also reads scans from other ASD instruments (a FieldSpec 3 and a Trek-type handheld so far); for
those, see section 4 for what is still provisional.

---

## 1. What SPYDER Bone does, and does not do

**What it does.** You give it `.asd` files from an ASD LabSpec 4 (or scan files from another ASD instrument, section 4).
For every scan it gives a verdict, a collagen reading,
a picture of the spectrum against reference bones of known collagen content, and short notes in plain words. You can
list scans newest first or most promising first.

**What it is for.** Choosing which spots to sample. The alternative is sampling at random, or sampling everything. An
imperfect screen is still a big improvement over guessing.

**What it does not do.** It does not replace extraction, dating or ZooMS. It does not measure collagen the way a lab
does; it estimates it from the spectrum. It says nothing about date, species or isotope values. It never asks you to
type in specimen data: there is nothing to fill in.

**Results are per scan, never per bone.** Bone varies from spot to spot, sometimes a lot. A verdict belongs to the
exact spot of one scan. Scan several spots on a bone and judge each scan on its own; never average them into one "bone
result". If one spot says Unlikely and another says Borderline, that is normal and useful: sample the better spot.

---

## 2. Installing and starting

**Download** the installer for your computer from https://github.com/makapania/NIR-SPYDER-Bone/releases/latest: the
`.exe` for Windows, the `.dmg` for a Mac (one file for both Apple Silicon and Intel Macs).

**Windows.** Run the installer. It installs for your user account only and includes the web component it needs. The
installer is digitally signed. Windows may still show a SmartScreen warning while this new app builds a download
history. Check that you downloaded it from the release page above and verify the publisher before proceeding.

**macOS.** Open the disk image and drag SPYDER Bone to Applications. It needs macOS 13 or newer. The app is signed and
notarized by Apple; macOS may ask you to confirm the first launch of an app downloaded from the internet.

**Starting.** The window opens ready to work, in a dark theme by default, with a light-theme button and a
**Colour-blind safe** button in the toolbar. There are no set-up screens. On macOS there is a small File menu (Open
Files…, Open Folder…, Watch Folder…, and Help); on Windows every action is in the toolbar. The **?** button (or F1) opens
this guide.

---

## 3. Two ways to work

**Open files or a folder.** Use **Open files** (Ctrl+O, ⌘O on a Mac) or **Open folder** (Ctrl+Shift+O, ⇧⌘O) and pick
scan files or a folder of them. Scan files are `.asd` files and, from a FieldSpec 3, numbered files (`name.000`,
`name.001` ...) and ViewSpec `.sco` files. Every scan gets its own row and its own verdict. Sub-folders are not read, only
the folder itself. You can also drag scan files, or one folder, onto the window. (In the Open files dialog, choose
"All files" to see FieldSpec 3 numbered files.)

A ViewSpec `.sco` file is the same scan as the numbered file it was made from, with ViewSpec's splice correction applied.
When both are in the folder (or opened together) only the numbered file is scored, so a scan is never counted twice. A
`.sco` on its own is scored; the collagen models read the same from either.

**Watch a folder live.** Click the **Watch folder…** pill and pick a folder: the LabSpec's save folder, or a drop
folder that scans are copied into (for example a shared or synced folder on a Mac, including network shares). New
scans are scored as they are saved and appear at the top of the list marked **NEW**. The pill shows the folder and its
state; you can pause, resume or stop watching (stopping keeps the scans already listed). The app resumes watching the
same folder when it is reopened, until you press Stop. The first time you watch a folder that already holds scans, the
app asks once whether to include them or only scans saved from now on. Network folders are checked on a timer instead
of instantly; the pill shows the interval.

A file that cannot be read is still listed, with the reason, so nothing silently disappears.

---

## 4. The Standard / High-res switch

LabSpec 4 instruments come in standard-resolution and high-resolution versions, and the two need different handling:
on high-resolution scans the app converts the spectrum to standard resolution before reading it (a "transfer"). The
toolbar has a **Standard / High-res** switch.

- If the app knows your instrument's serial number, the switch is preset for you, with a note saying so.
- If it doesn't, the app looks at the detector settings stored in the file (its SWIR gains, which differ between the
  two kinds of instrument on every unit we have seen) and presets the switch from those. This is only a good guess,
  based on five instruments.
- If neither gives an answer, the switch starts on **Standard** until you set it.
- The switch is always yours. If the preset is wrong, flip it; flipping it recomputes every scan, and your choice is
  remembered for that folder.
- If a file's serial or detector settings point to the other kind of instrument, a note in its details says so. It
  never changes results by itself.

The switch applies to every scan in the list, so keep scans from a standard and a high-res instrument in separate
folders and open them separately.

If you are not sure which kind of LabSpec 4 you have, ask whoever runs the instrument.

### Other ASD instruments (provisional)

The models were built on a standard-resolution LabSpec 4, but other ASD full-range instruments save very similar data,
and the app reads them:

| Instrument | Files | What is stored |
|---|---|---|
| LabSpec 4 (standard or high-res) | `.asd` (format "as8") | raw counts and the white reference |
| Trek-type handheld (ASD mineral-identification line) | `.asd` (format "as7") | raw counts and the white reference |
| FieldSpec 3 | `name.000`, `name.001` ... and ViewSpec `.sco` (legacy format) | reflectance only |

- **Use the Standard setting** for these instruments. Their detector settings point to Standard, so the switch is preset
  that way. Their spectral resolution is like a standard LabSpec 4 (FieldSpec 3: 3/10/10 nm at 700/1400/2100 nm; the
  Trek-type handheld is rated 3/9.8/8.1 nm), so no conversion is applied.
- **Detector joins are found per file.** The joins between the instrument's three detectors are not always at 1000 and
  1800 nm (the Trek-type handheld switches at about 1785 nm and leaves the header blank). The app uses the joins in the
  file header when they look right, otherwise finds them in the white reference, otherwise assumes 1000/1800 nm. The
  scan's details say which. The collagen models do not depend on the joins.
- **FieldSpec 3 files hold reflectance only**, so the saturation check cannot run on them ("not assessed"). A saved white
  panel is still caught as a white-panel scan.
- **How well the readings carry over is still being tested.** On a first test set (15 bones scanned on a FieldSpec 3 and
  on a Trek-type handheld, with measured collagen yields), the models ranked the bones well on both instruments and read
  the near-zero bones below 0.5%, but the readings ran low against the yields, by about half at high yields (an 18%
  bone read about 10%). Whether that comes from the instruments or from how the yields were measured is not settled.
  Until it is, treat readings from these instruments as provisional, especially in the 1–5% range.

---

## 5. Reading a result

### The verdict

One verdict per scan, shown as a word, a symbol and a colour. The symbol carries the meaning, so colour is only a help:

| Verdict | Meaning |
|---|---|
| **Good candidate** | Collagen looks ample for radiocarbon, isotopes or ZooMS. |
| **Borderline** | Collagen may be enough. Worth sampling if this bone matters. |
| **Unlikely** | Collagen is probably too low. Another area of the bone may do better. |
| **Can't tell** | The scan cannot be judged at this spot. A lighter or cleaner area, or more averages, may help. |
| **Rescan** | This scan cannot be read (the probe lifted, the detector saturated, or the probe was on the white panel or an empty holder). Rescan the spot. |
| **Doesn't look like bone** | The spectrum does not resemble bone. Check the target: the probe may be on plaster, a label, a coating or the holder. |

There is **one verdict for all analyses**: the app does not ask which analysis you are planning. ZooMS gets an extra
line where it applies (below).

Below the verdict you will usually see a collagen reading, such as "≈ 1.7% (± 1)". Very low readings are shown only as
"< 0.5%", and high ones coarsely (up to "> 6%"), because above a few percent the exact number does not change the
decision. The reading is the **consensus of three collagen models**, not a measurement.

Under it, in quiet grey, a **low-range estimate** (for example "Low-range estimate ≈ 1.2%") reads the same scan with a
model fitted only on bones below 6% collagen. Below about 4% it usually sits closer to the measured yield than the
consensus, which tends to over-read low bones; around 3–6% it commonly reads lower than the consensus, and above that
it reads well below the yield. It is a second number for the low end, shown for every scan. **It never sets the
verdict.**

### The collagen models

The details panel shows:

- **Consensus of three**: the median of the three models below it. This sets the verdict. Its typical error is about
  ± 1 point below 3%.
- **2045 nm, OH-corrected** (reads 2030–2060 nm), **1545 nm, OH-corrected** (reads 1500–1550 nm) and **N–H set, 2175 +
  2045 nm, OH-corrected** (reads 1500–1550, 2030–2060 and 2150–2200 nm): the three inputs to the consensus.
- **Ryder 2045** (Ryder et al. 2026; reads 2030–2060 nm): the published model, shown for reference. It does not set the
  verdict.
- On high-resolution scans only: **1545 nm, no transfer needed** (reads 1500–1550 nm), a second opinion that reads the
  scan exactly as measured. It does not set the verdict; if it differs from the main reading, a note says so.
- Two **panel readers**, which never set the verdict. **Low-range estimate (narrow 1545 nm band)** (reads 1533–1557
  nm) is the low-range estimate shown under the verdict (see above); it reads the 1545 nm N–H band as a narrow band
  against its shoulders.
- **C–H 1728 nm** (reads 1716–1740 nm) reads collagen from the C–H band at 1728 nm. On clean bone it agrees with the
  N–H models; lipids, waxes and many consolidants add C–H without nitrogen and raise it. The app compares it with the
  consensus for the C–H note (below).

"OH-corrected" means the model allows for differences in the OH/water bands at 1450 and 1930 nm, which vary between
bones as the bone's bound water and OH groups are altered over time. These bands are shaded on the charts. The
correction is a standard chemometric method, External Parameter Orthogonalization (EPO; Roger et al. 2003): the
direction in which altered OH/water moves the spectrum, measured on collagen-free bone, is removed before the model
reads the scan. At 2045 and 2175 nm it is mostly water held in the bone (the 1930 nm band comes only from water); at
1545 nm, water and OH together (the 1450 nm band).

### The ZooMS line

ZooMS can work on less collagen than radiocarbon needs. Where the collagen **bands** make a scan look better for ZooMS
than its verdict suggests, a line appears under the verdict, for example:

- "**Better chance with ZooMS**: all six collagen bands are resolved at this spot."
- "**Some chance with ZooMS**: protein bands show at this spot." (and similar)
- "**Faint sign for ZooMS**: only one protein band shows here, which is weak evidence on its own."
- On an Unlikely scan, a line also appears when the published Ryder 2045 model reads above the level its authors
  proposed for ZooMS.

The line helps you keep promising ZooMS spots in view. It never changes the verdict. In the scans list such rows carry
a small **ZooMS** tag. The line is not shown when a contaminant sign fired or the C–H note shows ("extra organic
material"), because coatings can create the bands it reads.

### Organic evidence and the collagen bands

The **Evidence of organics** section judges the bands directly, with no model: a level (**None / Trace / Clear /
Strong / Can't tell**), a row of dots (one per band), and a **ZooMS band check** that describes the band pattern in words
(for example "All six collagen bands resolved", or "Too noisy to read at 2044/2175 nm"). The bands are C–H at 1689 and
1728 nm; N–H with amide II at 2044 nm; the amide envelope at 2175 nm (several amide combination bands together);
a combination band at 2262 nm (its assignment is uncertain: the published O–H/bound-water assignment does not fit
the arithmetic, it is not a clean C–H band, and it does not follow the OH/water bands here; it rises with collagen in the
reference bones, and coatings can raise it too); amide III with CH₂ at 2284 nm;
and the bonded N–H band at 1545 nm, read on the OH-corrected window. The protein bands named in the ZooMS check are
2044 and 2175 nm; the 1545 nm band adds a separate vote.

Band evidence works both ways. Flat protein bands lower a verdict ("No protein signal at this spot"); strong, clean
bands on a scan with no contaminant sign can raise one.

### Flags

If the scan shows signs of a **lipid** (wax, for example, or fat; from any source: a coating, the bone itself or elsewhere), a
**consolidant (ester)**, **plaster**, another **foreign organic** (more C–H than the bone's collagen explains, or an organic
film such as a consolidant, marked on the spectrum where it was read), or **heat**
(charring or calcining), a flag appears beside the verdict and in the scans list, and a marker on the spectrum shows
where the heat sign came from. The flag tells you to check the spot. **Flags never change the verdict**: many collagen
models still read well on treated bone. But lipid, consolidant, plaster and foreign-organic signs stop the band evidence
from *raising* a verdict, because coatings can create the bands it relies on. For the same reason, in the **most
promising first** order a scan with one of those signs comes after the clean scans of the same verdict (a heat sign
does not move it).

### Notes

Under the verdict, up to two short notes explain what decided the result: for example that flat protein bands lowered
the verdict, that resolved bands raised it on a clean scan, that the scan was too noisy to check for contaminants, or
that the three models fall on different sides of a verdict line (the verdict then uses the middle value). Anything else
is listed under **Also noted** in the details panel.

Two notes compare readers that fail in different ways. **They never change the verdict.**

- **C–H above N–H.** On clean bone the C–H reader and the N–H consensus agree. When C–H reads higher than clean bone
  shows, the note says "Possible extra organic material"; when it reads far higher (rare on clean bone), "Likely extra organic material"
  (the bone's own fat, wax or consolidant, for example). It means C–H that collagen does not explain, not necessarily a
  contaminant: fat in the bone itself counts, so fresh or fatty bone can raise it, and old bone that has lost its collagen
  can keep some other organic matter. About 1 clean scan in 50 reaches the first level (some sites run a little higher
  than others); the second level is rare on clean bone. Animal glue adds both C–H and N–H, so it does not show here.
- **The collagen readers disagree strongly.** When readers that agree on clean bone end up much further apart than
  they ever do there, the note asks you to treat the percentage with caution. It does not name a cause: the spectrum
  says something is unusual, not what. When it shows, the plainer "models fall on different sides of a verdict line"
  note is not repeated.

The stronger of these, "Likely extra organic material" and "The collagen readers disagree strongly", also put a small
quiet tag in the scans list (**C–H** or **Readers**), so you can see them without opening each scan. At the bottom of
the details panel, **How this verdict was made** gives the scan's own numbers: the three consensus models and their
spread, how far the C–H reader sits from the consensus against the C–H note's two levels, the most unusual difference
between readers (the pair closest to the line where the disagreement note fires, each pair judged against its own line),
and the noise in the reading.

---

## 6. The spectrum views and close-ups

The main chart plots your scan against reference bones of measured collagen yield (0, 1, 3, 6 and 10%; the mean
spectra of public calibration bones from Ryder et al. 2026), dashed, each level toggleable. Buttons switch between
**Reflectance**, **Absorbance** and **2nd derivative** (the default). The 2nd derivative is drawn inverted
(−d²A/dλ²), so absorption bands point up and a deeper band is a taller peak; the small **Bands down** button beside the
axis label shows the conventional d²A/dλ² (bands point down, as in most papers) in every chart, and the app remembers
your choice. It changes the picture only, never a reading. A **Display smoothing** menu
changes the chart only; the models always use their own fixed smoothing. Shaded strips mark the model windows and the
OH/water bands.

On high-res scans the main chart shows the **transferred** spectrum (what every model reads), with **As measured** one
click away.

Two model-window close-ups sit beside the main chart: the **2045 nm model** window (showing the OH-corrected view the
model actually reads, with an **Uncorrected** toggle) and the **1545 nm model** window. Under **Collagen bands**, three
close-ups zoom onto the bands, each with a dashed 3% reference curve and a fixed vertical scale set by the 10%
reference, so a flat band cannot be blown up to look lit. Hovering a band in the details panel highlights it in the
close-ups.

---

## 7. Getting good scans

- **Use enough averages.** When a scan is too noisy to judge or to check for contaminants, the app suggests rescanning
  with 100–200 averages. Dark spots read noisier, and high-res instruments are noisier above about 1800 nm, most of all
  above 2300 nm. On a noisy high-res scan the plaster and general organic checks fall back to a reduced form read below
  1800 nm (the details say so). In tests it found most added plaster and can catch heavy consolidants, but it misses
  thin coats, glue and many consolidants. More averages may let the full checks run.
- **"Noise could change the verdict"** means the collagen reading sits within noise reach of a verdict cut (0.5% or
  3%). A rescan of the same spot with more averages settles it. Noise far from any cut is mentioned quietly under
  Checks: it does not change the verdict.
- **Take a white reference** as the instrument requires. White-reference saves are listed in the app but never scored.
- **Scan several spots per bone** and treat each scan on its own. Some spots are good, some are bad.
- **Keep the probe on clean, flat bone.** If a scan comes back **Rescan** ("Low signal across the whole range. The probe
  may have lifted. Rescan this spot."), do exactly that.
- **Rescan rather than trust a noisy scan.** Where noise prevents a check, the app says so rather than guessing.

---

## 8. Export

**Export CSV…** in the toolbar saves two files, both ready for Excel:

- **`name.csv`**, the one to read: one row per file in the list, worded as on screen. It gives the file, when it was
  scanned, the verdict, the collagen reading as shown, why, the ZooMS line, any flags, other notes, the protein-band
  level and the ZooMS band check, each model's reading, and the instrument setting. The citation is at the top.
- **`name (technical).csv`**, saved beside it: every value unrounded, with checksums, band readings and every quality
  check, for checking results or sending them to someone who works with the numbers. The command-line tool writes
  this file.

---

## 9. Limits and honest caveats

- **The reading is an estimate.** Typical error is about ± 1 point below 3%, and larger on bone unlike the bones the
  models were built on. When a spectrum is outside that range, the model's row says "Less familiar spectrum": read that
  number with more caution.
- **Other ASD instruments are provisional** (section 4): the readings ranked bones well in a first test but ran low
  against measured yields; there is no per-instrument correction yet.
- **The high-res conversion is approximate.** High-res scans are converted to standard resolution before the models read
  them, so the app also shows the 1545 nm model with no transfer needed as a cross-check.
- **Coatings and glue can make bone look better.** Thin coats of glue or lacquer can raise every collagen reading by
  1–4 points, and the foreign-organic sign catches only about half of them. Animal (hide) glue reads as the bone's own
  collagen: about half of glued scans trip the foreign-organic sign, but it is never named as glue. The contaminant
  checks are less certain on high-resolution scans. Treat "no sign found" as reassuring, not as proof. On a bone that
  may have been glued or coated, check its history and treat a Borderline reading with extra caution.
- **Readings higher than archaeological bone normally holds** get a note: modern bone, glue or another protein may be
  present.
- **Agreement is not proof.** Collagen-like contaminants can fool several readers together, so agreement between
  readers does not prove the collagen is uncontaminated. Animal glue is itself collagen: even a little of it lights the
  same protein bands the checks read, the ZooMS line included, before any warning appears. On bone that may have been
  glued, a positive reading or protein signal does not show that the protein is the bone's own.
- **The bands are suggestive, not proof.** Reading band patterns is a screening judgement.
- **An Unlikely verdict is about one scanned spot**, not the whole bone. Another spot may do better.

---

## 10. The command-line tool

For batch work there is also a command-line version, `spyder`, built from the source code (it is not part of
the desktop installer):

```
spyder read     <file.asd | folder> [--json] [--recursive]
spyder validate <plug-in file | folder> [--json]
spyder predict  <file.asd | folder> --class std|hires [--json]
spyder analyse  <file.asd | folder> --class std|hires [--profile radiocarbon|isotopes|zooms]
                [--csv out.csv] [--json] [--sort] [--recursive]
```

`spyder analyse` runs the full per-scan pipeline and writes the same CSV as the app's Export button. The tool's README
in the source repository has the details.

---

## 11. Citing and licences

The application code is MIT-licensed. The bundled model, transfer, reference and parameter files are CC-BY-4.0.

The collagen models are built on the calibration data of:

> Ryder, C. et al. 2026. Refining near-infrared spectroscopy for collagen quantification in archaeological bone.
> *Journal of Archaeological Science* 185:106448. doi:10.1016/j.jas.2025.106448

Every exported CSV carries a citation line.

---

## 12. Troubleshooting / FAQ

**A scan says Rescan. What now?**
Read the reason under the verdict (probe lifted, detector saturated, or the probe was on the white panel or an empty
holder) and rescan the spot.

**A scan says "Doesn't look like bone".**
The probe is probably not on bone: plaster, a label, a coating or the holder. Move the probe and rescan.

**My file is listed as "Not readable".**
The app reads ASD `.asd` files (LabSpec 4 and Trek-type handhelds) and FieldSpec 3 reflectance files (`.000` ... and
`.sco`), and lists anything else with a reason. Re-save or re-export the scan on the instrument. FieldSpec 3 files saved
as raw counts (not reflectance) cannot be scored: save them as reflectance.

**The verdict says Can't tell, or the bands are too noisy.**
Rescan with more averages (100–200), or pick a lighter or cleaner spot.

**A note says my switch setting disagrees with the file's serial or detector settings.**
Flip the Standard / High-res switch to match your instrument. It is remembered for the folder.

**The switch was set for me. Can I trust it?**
A known serial number is reliable; a preset "from the detector settings" is a good guess. If you know your instrument,
set the switch yourself.

**A row says "Changed".**
The file changed on disk after it was scored (it was saved again). The app now shows the new version.

**Watching stopped finding new scans.**
The pill shows **Paused** (press play), **Waiting for** (the folder is unavailable; watching resumes by itself when it
comes back), or **Watch folder…** (watching was stopped; start it again).

**The status bar says "No verdict model available".**
The bundled model files failed to load. Files are listed but not scored. Reinstalling restores them.

**Why do the three models disagree?**
It is common on coated or treated bone, or bone with an altered OH/water band. The verdict uses the middle value.

**Can I combine scans of one bone?**
No. Every result is per scan. Scan several spots and judge each scan on its own.
