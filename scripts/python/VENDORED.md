# Vendored Python libraries

These libraries are vendored so the dmg layout generator can run offline on
macOS CI runners without `pip install`:

| Package | Version | Source | License |
|---|---|---|---|
| `ds_store` | 1.3.3 | https://pypi.org/project/ds-store/ | MIT (`ds_store/LICENSE`) |
| `mac_alias` | 2.2.3 | https://pypi.org/project/mac-alias/ | MIT (`mac_alias/LICENSE`) |

`ds_store` imports `mac_alias`; both are pure Python and are kept at the
versions published on PyPI. Do not edit vendored sources in place; update by
replacing the package directory and this manifest, keeping the license files.

`gen_dmg_ds_store.py` prepends this directory to `sys.path` so imports resolve
to the vendored copies regardless of the caller's working directory.
