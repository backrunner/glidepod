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

## Construction

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
