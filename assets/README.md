# Application icon

`GlidePod.icon` is the primary, editable Icon Composer document. Open it in the Icon Composer supplied with Xcode 27. It reconstructs the selected B glass design using two groups and five SVG layers: two square package faces, their fine contours and the foreground opening. The forest background is a fixed solid fill. See the [reconstruction record](../.agents/design/icon-composer/README.md) for geometry, previews and validation.

The source contains precise vector geometry and explicit alpha gradients for clean glass overlap. Apple's renderer supplies the system mask, specular treatment and restrained shadows. Strong refraction is disabled to avoid distorting the overlap. No generated raster texture, baked background mask, letters or Apple artwork is included. Keep the forest, sage and ivory palette, the foreground opening, equal square proportions and centered composition when refining the geometry. Dark appearance has explicit color and alpha overrides.

`tools/package-dev.py` compiles the document with `actool`, then includes `Assets.car`, the generated ICNS fallback and both `CFBundleIconName`/`CFBundleIconFile` keys. Do not replace this with a flattened PNG-only icon. The older `glidepod-icon.svg` is the historical flat prototype mark, not the packaged application icon.

Run `rtk proxy python3 tools/export-icons.py` to render the six macOS appearances and 16/32/64/128-pixel previews with design generation 27. These PNGs are review evidence, not bundle inputs. Xcode 27 is required for this renderer; compilation targeting macOS 15 does not establish runtime validation on macOS 15.

References: [Icon Composer](https://developer.apple.com/icon-composer/), [creating an app icon](https://developer.apple.com/documentation/xcode/creating-your-app-icon-using-icon-composer), [Apple app icon guidance](https://developer.apple.com/design/human-interface-guidelines/app-icons).

Original artwork is licensed under Apache-2.0 with the project.
