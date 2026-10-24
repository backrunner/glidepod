# Icon Composer reconstruction

Date: 2026-10-23. Status: implemented in the editable master and local development bundle.

## Design reference

The selected B clear-glass concept was refined with a quieter backing. This reconstruction replaces the generated background and foreground pixels with deterministic vector artwork and Icon Composer settings. It follows the selected composition and palette; it is not a pixel-exact trace of the generated lighting. Discarded concept images remain local; the current editable source and rendered previews are published here.

## Files

- Production master: [assets/GlidePod.icon](../../../assets/GlidePod.icon)
- Design copy: [GlidePod.icon](GlidePod.icon)
- [Default preview](preview-default.png), [dark preview](preview-dark.png), [32-pixel preview](preview-default-32.png)
- [Earlier construction view in Icon Composer, before the slot correction](icon-composer.jpg)
- [Validation and source hashes](validation.json)

## Original construction — 2026-10-23

Two groups contain five SVG layers: a sage rear face, its edge contour, an ivory foreground face, its edge contour, and the dark package opening. The front face has a real aperture in its vector path. The separate dark insert preserves the opening's contrast through appearance changes.

Both body shapes are 440 by 440 on a 1024-unit canvas with an 88-unit corner radius. Their centers are (438, 438) and (586, 586), with rotations of -12 and +12 degrees. A shared 1.07 scale around (512, 512) brings the mark to the selected reference's visual size: each square is 470.8 units wide and high. The symmetry of the body bounds keeps the composition geometrically centered.

The slot was corrected on 2026-10-23 against the original package mark. In the front square before rotation, its center is 40% across and 30% down; its capsule is 172 by 34 units. The earlier reconstruction incorrectly placed it at 48% across and 22% down with a 220-unit width. The face aperture, both rim paths, and dark insert now share the corrected geometry. Square bounds, centering and material settings are unchanged.

The background is a single solid forest fill, approximately #315E4B. No generated image, texture, noise or baked background mask is included. The faces and fine contours use explicit SVG gradients to control transparent overlap and edge definition. Apple's renderer supplies its system mask, specular treatment and restrained shadows. Strong dynamic refraction was disabled after visual trials because it distorted the rear contour in the overlap. Glass effects are disabled on the fine contours and the opening insert to keep those details controlled.

Dark appearance has explicit color/alpha overrides so that the opening remains readable and both faces remain transparent. Clear and tinted appearances follow the system renderer. Source artwork remains editable at any resolution.

The system still supplies its own perimeter lighting and pixel quantization. A fixed solid source does not imply every exported background pixel is identical; the measured interior review patch varies by only 1-2 levels per RGB channel. The generated background's irregular texture is absent from the source.

## Verification and integration

The slot-corrected default, dark and 32-pixel exports were inspected. The new bundle was compiled, signature-checked and launched. Native window interaction after this correction is pending because the desktop was locked; the user asked to preserve the current result. The Icon Composer screenshot below is historical construction evidence, not a screenshot of the corrected slot.


- Opened the final master in Xcode 27 Icon Composer; confirmed design generation 27 and five layers in two groups.
- Exported default, dark, clear light/dark and tinted light/dark appearances, plus default sizes 16, 32, 64 and 128.
- Visually inspected the default, dark and appearance previews, including the 32-pixel mark. This is visual review, not an accessibility certification or a claim that every tint/wallpaper combination has been tested.
- Compiled with actool into Assets.car and the ICNS compatibility fallback; both bundle icon keys reference GlidePod.
- The original reconstruction was packaged without Rust changes. The slot correction is packaged together with experimental runtime management; see the current implementation record for code and live VM verification.
- No release, notarization, global installation or publication was performed.

Re-export using rtk proxy python3 tools/export-icons.py. Repackage using rtk proxy python3 tools/package-dev.py --skip-build when the development binaries are current.

The editable-layer workflow and appearance controls follow Apple's [Icon Composer documentation](https://developer.apple.com/documentation/xcode/creating-your-app-icon-using-icon-composer). The exact materials and geometry here are GlidePod artwork.

## Interface integration — 2026-10-24

The GPUI sidebar, interactive prototype (sidebar, dock, menu bar and favicon), README and icon preview now use light/dark exports of this corrected master. `tools/export-icons.py` writes the 256-pixel assets to `assets/brand/` and synchronizes the prototype copies. The obsolete early raster in `.agents/media/` was removed. The app bundle continues to compile the editable `.icon` document into `Assets.car` and `GlidePod.icns`; interface PNGs do not replace the layered macOS icon.

## Retina refinement — 2026-10-24

The 40-point native sidebar now uses explicit 40/80/120-pixel renditions selected from the window backing scale, instead of bilinearly shrinking a 256-pixel preview without mipmaps. The prototype sidebar uses matching density descriptors. Both appearances are exported directly from the vector master at each target size.

The package contours now use a single 4-unit stroke. The previous 11-unit, low-opacity halo was removed and the 2.4-unit core was strengthened, retaining the geometry, glass faces, palette and overlap while narrowing the edge transition. Both editable copies contain this refinement. The earlier reconstruction validation record remains historical; the current review is recorded in the native UI audit.

The layered catalog already contains Retina renditions through 1024 pixels. Separately, packaging now builds a complete ICNS fallback from that catalog, retaining its system inset and lighting; actool's partial fallback previously stopped at 256 pixels. The bundle directory's modification date is refreshed after signing so local icon changes are discoverable.

## Emerald conic background — 2026-10-24

The user found the flat forest tile too dim beside other Dock icons. The first brightness pass replaced it with a brighter emerald conic gradient, centered at (0.5, 0.5), rotated -95 degrees, with a softened center of radius 0.22. The ordered color stops are `#15966E`, `#65C99A`, `#146F64`, `#249F91`, and the starting color again. `assets/icon-background.json` is the editable color-field source; `tools/generate-icon-background.swift` produces the continuous 2048-pixel image used in the third, non-glass group. There are now five vector layers and one background image in three groups. System masks and appearance treatments remain native.

The rear face is mint `#BCEBD3` at alpha 0.84/0.54/0.72; the ivory front is `#F6FFF6` at 0.98/0.76/0.92. The opening is deep green `#154C3D`, and group shadow opacity is reduced from 0.18 to 0.10. These changes improve foreground separation at Dock sizes without changing square geometry, slot placement or the 4-unit contours. Dark appearance retains an emerald backing and stronger face contrast. This palette was subsequently refined below; the previews above always reflect the latest exported source.

The export command synchronizes the two editable documents, density-specific interface resources, prototype and review previews. Local comparison and native evidence are recorded in `evidence/icon-brightness-2026-10-24/`. Asset previews are explicitly distinguished from native screenshots.

## Center-lit radial refinement — 2026-10-24

The user preferred a bright center and darker surroundings. The first radial pass used a centered gradient, superseding the conic version above. Center: (0.5, 0.5), radius: 0.64 of the canvas width. Color stops at 0/0.45/0.8/1 are `#6ACCA0`, `#41B58C`, `#23866B`, and `#176351`, with smooth interpolation. The mark, face opacity, contours and opening remain unchanged. The generated layer is `00-gradient-background.png`; all active exports and both editable documents use it. The bitmap matches the 1024-unit canvas, supplying the full 512pt @2x resolution. The former untransformed 2048px background was center-cropped by Icon Composer; the generator now rejects that size so the radial perimeter cannot be lost. The dark appearance retains its existing deep emerald specialization. Local evidence: `evidence/icon-radial-2026-10-24/`.

## Restrained radial lighting — 2026-10-24

The first center-lit result looked too bright in the actual application icon. Three lower-intensity variants were exported at 512 and 80 pixels; the balanced middle variant was selected after visual review. It retains a centered radial field but expands the radius to 0.68 and uses `#66AD8C`, `#439978`, `#2C765F`, and `#215E4C` at positions 0/0.45/0.8/1. This reduces the luminous green ring around the faces while keeping the perimeter darker than the center.

The rear face is now jade `#B5DAC5` at alpha 0.82/0.52/0.70; the front is soft ivory `#E8F6EC` at 0.96/0.72/0.88. The 4-unit contour remains sharp, with endpoint opacity lowered to 0.86/0.72. The opening, geometry, material settings, dark specialization and Retina export dimensions are unchanged. Both editable documents and native/prototype resources use these values. Candidate previews and actual application captures are recorded separately under `evidence/icon-balanced-2026-10-24/`.
