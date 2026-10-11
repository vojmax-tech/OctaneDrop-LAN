# -*- mode: python ; coding: utf-8 -*-
from pathlib import Path

from PyInstaller.utils.hooks import collect_data_files, collect_submodules

project_root = Path(SPECPATH)

datas = collect_data_files(
    "app",
    includes=[
        "templates/*",
        "templates/**/*",
        "static/*",
        "static/**/*",
    ],
)
datas += [
    (str(project_root.parent / "README.md"), "."),
    (str(project_root.parent / "LICENSE"), "."),
]

hiddenimports = collect_submodules("uvicorn") + collect_submodules("websockets") + collect_submodules("webview")


a = Analysis(
    [str(project_root / "launcher.py")],
    pathex=[str(project_root)],
    binaries=[],
    datas=datas,
    hiddenimports=hiddenimports,
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=[],
    noarchive=False,
    optimize=0,
)

# Work around a PyInstaller extraction issue caused by a random-named mypyc
# extension present in some user-level site-packages environments.
# The app does not require this optional binary.
a.binaries = [entry for entry in a.binaries if "__mypyc.cp314-win_amd64.pyd" not in entry[0]]
pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.datas,
    [],
    name="VoltDrop",
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=True,
    upx_exclude=[],
    runtime_tmpdir=None,
    console=True,
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
    icon=str(project_root / "packaging" / "voltdrop.ico"),
)


