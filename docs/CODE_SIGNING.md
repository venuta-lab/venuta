# Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io), certificate by
[SignPath Foundation](https://signpath.org).

## What is signed

The Windows installer (`Venuta_x.y.z_x64-setup.exe`) and the program in it, built from this
repository by the [release workflow](../.github/workflows/release.yml) on GitHub Actions. Nothing
built anywhere else is signed with this certificate.

The installer also carries [yt-dlp](https://github.com/yt-dlp/yt-dlp), an open-source tool
published by its own authors. It is included as they release it and is not signed by this
project.

## Team roles

| Role | Who |
|---|---|
| Committers and reviewers | [@theSelfSa](https://github.com/theSelfSa) |
| Approvers (each signing request) | [@theSelfSa](https://github.com/theSelfSa) |

Every change reaches `main` through a reviewed commit, and every release is approved by hand
before it is signed.

## Privacy

See [PRIVACY.md](../PRIVACY.md). Venuta has no telemetry. It connects to the internet only for
what the user asks for (lookups, the online catalog, playlist links) and a daily update check that
can be switched off.

## Updates

Venuta's in-app updater installs only releases signed with the project's own update key; an
update that doesn't match the key built into the app is refused.
