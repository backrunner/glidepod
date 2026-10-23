# Experimental renderer patches

`moltenvk-dynamic-triangle-fan.patch` applies to MoltenVK 1.4.2, commit `db66022459ffb663aa2b50f6b018bc2e124f5edf`. The upstream files are Apache-2.0; their [license](MoltenVK-LICENSE.txt) and [copyright attribution](MoltenVK-NOTICE.txt) accompany this patch. The build also copies the original license and dependency notices into the external renderer's `licenses` directory. This is a GlidePod patch, published here but not submitted or accepted upstream.

MoltenVK converts triangle fans into indexed triangle lists because Metal does not have a fan topology. Its four draw paths selected that conversion from the pipeline's original topology, even when `vkCmdSetPrimitiveTopology` changed the effective topology. Both a triangle list and a triangle fan become `MTLPrimitiveTypeTriangle`, so the existing Metal state could not recover the distinction. With ANGLE's dynamic state, only the first triangle appeared.

The patch retains the Vulkan topology in static and dynamic render state, resolves it with the same state-selection rule as the other dynamic properties, and uses the effective topology in direct/indexed/indirect fan conversion. It leaves the public feature advertisement and shader interpolation semantics unchanged. Last-vertex flat interpolation comes from MoltenVK's optional private Metal implementation, not a fabricated extension.

The original Android fixture in `android/benchmark/src/dev/glidepod/benchmark/Gles3Checks.java` covers all four fan draw paths, surrounding topology changes, last-vertex interpolation, index ordering, primitive restart, instancing and other GLES 3 features. The unpatched 1.4.1 and 1.4.2 failures and patched successes are retained in `evidence/g5`. These targeted checks do not establish Vulkan/GLES conformance.

`tools/build-metal-renderer.py` verifies `engine/metal-renderer-lock.json`, applies exactly this patch, builds an arm64 release library and creates an immutable cache directory. It does not overwrite the old renderer, register the candidate automatically, or bundle third-party runtime libraries into GlidePod.app.
