# Sources for Wingmate Android Local

The matching release at https://github.com/alphagauss/wingmate/releases contains the Wingmate source, the build scripts and patches, and the Android runtime source archive. The Local APK combines the Wingmate Android app with the dsh-Go local-agent module under GNU AGPLv3. See `apps/mobile/README.md` for instructions to build, sign with your own key and install a modified APK.

## Bundled runtime

- **PRoot, talloc and libandroid-shmem**. The runtime source archive contains their source archives and the Termux packaging tree. Wingmate's `runtime/android/build-runtime.sh` records the ELF changes applied after extracting the Termux packages.
- **Ubuntu Base and additional packages**. The runtime source archive contains the corresponding Ubuntu source packages under `ubuntu/`. Each `.dsc` names the source archive and any Ubuntu patches. The installed guest retains package copyright notices under `/usr/share/doc` and license texts under `/usr/share/common-licenses`.
- **DeepSeek Harness 0.2.2-rc.1**. Source and upstream build scripts at https://github.com/deepseek-ai/deepseek-harness/tree/66491b05e3f05ad35bcdd4f6cd7ed26041d9e3db. Wingmate's npm lock records the dependencies and `runtime/patches` and `runtime/android/patches` contain the local changes.
- **Node.js 24.22.1 and pnpm 11.28.5**. Sources at https://nodejs.org/dist/v24.22.1/node-v24.22.1.tar.xz and https://github.com/pnpm/pnpm/tree/v11.28.5. Their licenses are in the guest.
- **Python 3.12.15**. The runtime source archive contains CPython and the python-build-standalone recipe and patches. The recipe's `pythonbuild/downloads.json` lists exact source URLs and checksums for the build inputs.
- **Sharp libvips 1.3.4**. The runtime source archive includes Cairo and the Sharp libvips build scripts and notices. The POSIX build script gives the exact source download paths and applies upstream patches. The package's `versions.json` and README in the guest record the bundled versions and licenses. Build the replacement shared library using those scripts and substitute it for `/opt/dsh/node_modules/@img/sharp-libvips-linux-arm64/lib/libvips-cpp.so.8.18.7` in the guest.
