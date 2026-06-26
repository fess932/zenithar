# platforms/

Platform-specific native config and patches, kept out of `src-tauri/` so each
platform's stuff lives in one place.

```
android/patch.sh        Re-applies our Android customizations after `tauri android
                        init` (which regenerates gen/android each build):
                          • RECORD_AUDIO + MODIFY_AUDIO_SETTINGS in the manifest
                            (mic for WebRTC voice calls)
                          • MainActivity.kt — session-cookie persistence via
                            SharedPreferences (save on stop / restore on launch)
                        Wired into `make app-android*` and the CI android job.

macos/Info.plist        Merged into the app's Info.plist. Mic/camera usage strings
                        (NSMicrophoneUsageDescription) for the macOS prompts.
                        Tauri only reads Info.plist from src-tauri/, so the mac make
                        targets copy this there at build (the copy is git-ignored).

macos/Entitlements.plist  Referenced by tauri.conf `bundle.macOS.entitlements`.
                        audio-input / camera / network.client — without these the
                        hardened runtime blocks the mic.
```

The `zenithar://` deep-link intent-filter is NOT here: Tauri generates it from
`plugins.deep-link.mobile` in tauri.conf.json.
