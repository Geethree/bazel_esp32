"""Expose a generated file built for the execution platform."""


def _exec_file_impl(ctx):
    return DefaultInfo(files = depset([ctx.file.src]))


exec_file = rule(
    implementation = _exec_file_impl,
    attrs = {
        "src": attr.label(
            allow_single_file = True,
            cfg = "exec",
            mandatory = True,
        ),
    },
)
