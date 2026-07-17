"""Hermetic host tools used by ESP32 development rules."""

_ESPFLASH_VERSION = "4.5.0"

_ESPFLASH_ARCHIVES = {
    ("linux", "aarch64"): struct(
        sha256 = "2d5972b9c18fc89bf253e60fe6df6a4f8db3aee5db0166b2c97b53bd21c01f09",
        triple = "aarch64-unknown-linux-gnu",
    ),
    ("linux", "x86_64"): struct(
        sha256 = "dcd7fd822f247df18966935bbb10c7cb4b700c87172349633e9e323ee2542717",
        triple = "x86_64-unknown-linux-gnu",
    ),
    ("mac os x", "aarch64"): struct(
        sha256 = "6614ff70e523a6bce5f4ccc6459b77275f5e7e900429004bb7eec463c95db28a",
        triple = "aarch64-apple-darwin",
    ),
    ("mac os x", "x86_64"): struct(
        sha256 = "3c5cb664742d883e4304d4fc611fc875b27a8f8d7d105d22da2f615eb36888a0",
        triple = "x86_64-apple-darwin",
    ),
}

def _normalize_arch(arch):
    if arch in ("amd64", "x86_64"):
        return "x86_64"
    if arch in ("aarch64", "arm64"):
        return "aarch64"
    return arch

def _espflash_repository_impl(rctx):
    platform = (rctx.attr.host_os.lower(), _normalize_arch(rctx.attr.host_arch))
    archive = _ESPFLASH_ARCHIVES.get(platform)
    if not archive:
        fail("espflash {} has no pinned binary for host {}/{}".format(
            _ESPFLASH_VERSION,
            platform[0],
            platform[1],
        ))

    rctx.report_progress("Downloading espflash {} for {}".format(
        _ESPFLASH_VERSION,
        archive.triple,
    ))
    rctx.download_and_extract(
        canonical_id = "espflash-{version}-{triple}".format(
            version = _ESPFLASH_VERSION,
            triple = archive.triple,
        ),
        sha256 = archive.sha256,
        type = "zip",
        url = "https://github.com/esp-rs/espflash/releases/download/v{version}/espflash-{triple}.zip".format(
            version = _ESPFLASH_VERSION,
            triple = archive.triple,
        ),
    )
    chmod = rctx.execute(["chmod", "+x", "espflash"])
    if chmod.return_code:
        fail("failed to make espflash executable: {}".format(chmod.stderr))
    rctx.file("REPO.bazel", "")
    rctx.file(
        "BUILD.bazel",
        """\
package(default_visibility = ["//visibility:public"])

exports_files(["espflash"])
""",
    )

espflash_repository = repository_rule(
    implementation = _espflash_repository_impl,
    attrs = {
        "host_arch": attr.string(mandatory = True),
        "host_os": attr.string(mandatory = True),
    },
    doc = "Downloads a pinned espflash executable for the Bazel host.",
)
