# Measurements of this fork's own changes

What each change of this fork measured on one PC, with the commit that made it. Self-reported;
nobody else has reproduced these yet.

- **PC:** RTX 4060 Ti 8 GB (driver 621.40), i7-13700K, 64 GB, Windows 11.
- **Build and settings:** release, `--console series-s`, 1920x1080 window, the settings at the end of
  the [README](../README.md).
- **Scenes:**
  - *opening cutscene*: the 3D part of the opening, 40 to 75 s after the window opens; the mean of
    the frame lines the run log writes every 5 s;
  - *prison walk*: Lucia's prologue; a script holds the movement and camera keys for three minutes
    through `VICTORY_KEYS_FILE` (no focus, no mouse), the same route every run; the mean of the
    frame lines while the walk goes on, and the lowest of them.
- **Noise:** two runs of the same build differ by about 1.5 fps, so a difference smaller than
  that says nothing. Most rows are one run each.

## Frame rate, before and after

| Change (commit) | Scene | Before | After |
|---|---|---|---|
| The ray-tracing skinning passes (30 `CSComputeSkinning*` programs) are left out while every ray is answered as a miss ([ab4571df](https://github.com/edsuuu/VI-BR-PORT/commit/ab4571df), [1556de82](https://github.com/edsuuu/VI-BR-PORT/commit/1556de82)) | opening cutscene | 6.8 fps | 10.9 fps |
| Card steps at stage 2 with the relay, as the default ([46f467a0](https://github.com/edsuuu/VI-BR-PORT/commit/46f467a0)) | opening cutscene | 12.4 fps | 16.3 fps (stage 1: 14.8) |
| Title buffer copies kept in the card's memory from 1 MiB up to 1 GiB on an 8-12 GB card ([389b9747](https://github.com/edsuuu/VI-BR-PORT/commit/389b9747)) | opening cutscene | 17 fps | 18-19 fps |
| The title's 32-bit graphics heap used in place, no host copy ([6349f8c0](https://github.com/edsuuu/VI-BR-PORT/commit/6349f8c0), [2aa1895d](https://github.com/edsuuu/VI-BR-PORT/commit/2aa1895d)) | opening cutscene | 18.2 fps | 21.6 and 24.4 fps (two runs) |
| The same | prison walk | 20.7 fps | 22.6 fps |
| The air's light bounded with the constant ambient ([557d2f03](https://github.com/edsuuu/VI-BR-PORT/commit/557d2f03), [31b9a82f](https://github.com/edsuuu/VI-BR-PORT/commit/31b9a82f)) | prison walk | 22.6 fps | 24.8 fps |
| Submissions prepared by a thread of the layer's own (`VICTORY_ASYNC_SUBMIT=1`, not the default) | prison walk | 22.9 fps (lowest line 14.4) | 24.3 fps (lowest line 18.6) |

The prison walk went from about 14 fps on 25 Nov to 24.0 fps on 26 Nov (lowest line 16.3);
standing still in the cell the title holds its cap, 30.0 fps (33.3 ms).

## Tried and not kept (measured, no gain or a worse picture)

| Change | Scene | Result |
|---|---|---|
| Half render scale (`VICTORY_RENDER_SCALE=0.5`) | opening cutscene | 12.6 to 13.3 fps: the card is not limited by pixels |
| The ray-traced shadow passes left out (`VICTORY_SKIP_RT_SHADOW=1`) | prison walk | 21.9 fps against 22.6: nothing |
| More room for textures on the card (`VICTORY_TEXTURE_ROOM_PERCENT=97`) | prison walk | 20.8 fps: nothing, and textures still wait |
| The BVH builder with real ray queries | prison walk | 12.7 fps against 20.7 |
| Indirect light from the probes without rays (`VICTORY_AMBIENT=<number>`) | prison | posterized walls and skin: replaced by a constant, faint ambient |

## Picture fixes of this fork (no frame-rate claim)

- Faces' mouths and expressions: 16-bit buffers of the facial network were read as zeros
  through descriptor tables.
- The opening video no longer steps back between frames: each list has its own upload buffer for
  movie planes written by the CPU ([fd36f4a4](https://github.com/edsuuu/VI-BR-PORT/commit/fd36f4a4)).
- A new texture shows in a slot only after its upload ([44ead3c5](https://github.com/edsuuu/VI-BR-PORT/commit/44ead3c5)):
  a reused pool piece showed old bytes (a green and blue checkerboard on the car seat of the opening).
- The Series S present of several planes (world and interface) is composed as the console's
  display does ([57a8a722](https://github.com/edsuuu/VI-BR-PORT/commit/57a8a722)), sent ahead without
  waiting for the card ([511f629a](https://github.com/edsuuu/VI-BR-PORT/commit/511f629a)).
- The decoration bake's 51 programs are known by name ([10ac68f9](https://github.com/edsuuu/VI-BR-PORT/commit/10ac68f9)):
  faces of the cutscene came out black.
- Tracing against the ray structures and the ray-hit shading are left out while every ray misses
  ([24f8d2ed](https://github.com/edsuuu/VI-BR-PORT/commit/24f8d2ed), [f1e5e454](https://github.com/edsuuu/VI-BR-PORT/commit/f1e5e454)): same picture.

## In progress on the development branches (26 Nov, not on main yet)

- `alphaResolve CS_AlphTransmissionPrefilterMip`, dispatched every frame and refused by 32-lane
  cards (it asked for 64-lane waves), translates for waves of 32: the window glass of the prison is
  drawn again.
- 35 compute programs of lighting, shadows and fog are translated approximately on 32-lane cards
  (among them the one that builds the light lists per cluster); the exact wave-wide form is under test.
- The card reads about 430 MB a frame in the layer's own compare passes; reducing that is the next
  frame-rate item, with the tree instance culling program (5 ms a frame for a handful of groups).
