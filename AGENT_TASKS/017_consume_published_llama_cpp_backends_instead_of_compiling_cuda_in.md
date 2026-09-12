# 017 — Stop compiling CUDA in; consume published llama.cpp backends at runtime

Status: RESEARCHED 2026-09-12 — architecture understood, nothing implemented
Goal (user's words): *"leave the dependencies for the user to deal with, not the app"*

## The problem this solves

The installer is 989.7 MB because CUDA kernels are statically linked into **every** binary
(~157 MB each × 7). See [[016_installer_bundles_every_binary_in_target_release]] for the
packaging half. This task is the deeper fix: don't compile CUDA in at all.

## Key finding — the artifacts already exist, published and versioned

Every `ggml-org/llama.cpp` release publishes prebuilt, per-platform, per-GPU-tier archives:

```
llama-b10405-bin-win-cuda-13.3-x64.zip     llama-server.exe + ggml/llama DLLs
cudart-llama-bin-win-cuda-13.3-x64.zip     cudart64_*, cublas64_*, cublasLt64_*
```

So **neither the ~331 MB backend nor the ~752 MB NVIDIA runtime has to be built or hosted
by us.** Atomic Chat compiles no backend for its default provider — it resolves a tag and
downloads these.

## Does an installed CUDA toolkit make the vendor package unnecessary?

Partly, and this is the precise answer to the question that started this. From Atomic Chat's
`scripts/download-llamacpp-cudart-windows.ps1`:

> "the CUDA Toolkit runtime DLLs … live in companion `cudart-llama-bin-win-cuda-{X.Y}-x64.zip`
> archives on the same release. **Without those DLLs, `llama-server.exe --list-devices`
> returns an empty device list on machines that don't have the CUDA Toolkit installed
> system-wide.**"

So a machine **with** the toolkit (this one: CUDA 12.0/12.3/12.8, 561 MB) does not need the
cudart package. A machine **without** it does. Both LM Studio and Atomic Chat ship it anyway
rather than depend on the host — LM Studio's copy on this machine is 752.4 MB
(`cublasLt64_12.dll` alone is 643 MB), and its manifests version it as
`vendor_lib_package_names: ["win-llama-cuda12-vendor-v2"]`.

Shipping a pinned copy is defensible: the host's `cublasLt` version is not interchangeable
across CUDA majors, the toolkit may not be on `PATH`, and most users have none. Probing the
host first and downloading only on miss is the option that actually honours the goal.

## What a resolver needs (ours has none of it)

`crates/llama-chat-web/src/routes/model/backend_install.rs:13` is a single hardcoded URL to
one `ggml-cuda.dll` in our own releases. Compare `scripts/resolve-upstream-backend.mjs`,
which emits `TAG / BACKEND / ASSET / URL / SHA256 / SIZE` from a remote manifest:

1. **Tag resolution** — the manifest is authoritative, so the backend always matches the
   app's llama.cpp version. Ours breaks the moment the submodule pin moves, and an
   ABI-mismatched `ggml-*.dll` is exactly the kind of failure that presents as a silent CPU
   fallback rather than an error.
2. **`sha256` + size per asset.**
3. **A mirror with upstream fallback** — they hit a race where a tag was marked latest
   before its assets finished uploading (their ATO-95).
4. **No hardcoded CUDA minor** — it "drifts release to release" (their ATO-174); derive it.
5. **Runtime tier selection** with driver gating and graceful degrade to CPU.

## The blocker for us, stated honestly

Atomic Chat drives `llama-server.exe` as a **subprocess** over HTTP. We link llama.cpp
through Rust bindings (`deps/llama-cpp-rs`). Their archives therefore cannot be consumed
as-is.

What we would need instead:
- Build with `dynamic-backends` (`GGML_BACKEND_DL=ON`, `llama-cpp-sys-2/build.rs:803`), which
  makes ggml discover `ggml-*.dll` at runtime — `model_manager.rs:153` already calls
  `backend.load_all_backends()`.
- Obtain a `ggml-cuda.dll` whose ABI matches the exact llama.cpp commit our bindings were
  generated against. **Unverified:** whether ggml-org's published DLLs can satisfy our
  pinned submodule, or whether we must build that one DLL ourselves and host it.

That single unknown decides the whole shape:
- **If upstream DLLs work** → tiny installer, no build/hosting burden, just a resolver.
- **If not** → we build `ggml-cuda.dll` per llama.cpp bump and host it with a manifest,
  which is still far better than the status quo but is real release infrastructure.

## Concrete next steps

- [ ] Build once with `--features vision,dynamic-backends` (no `cuda`). Measure the binary
      and confirm the cmake output produces `ggml-*.dll` (`build.rs:1468` scans for them).
- [ ] Check whether a model loads on GPU with only that DLL present, using the machine's
      existing CUDA toolkit — that single experiment answers the vendor-package question for
      hosts that already have CUDA.
- [ ] Determine ABI compatibility with a published ggml-org DLL of the same tag. **This is
      the gating question; answer it before designing a resolver.**
- [ ] Only then: manifest + tag/sha256 resolution, replacing the hardcoded URL.

## Related

[[016_installer_bundles_every_binary_in_target_release]] — the packaging half. If 017 lands,
016 mostly evaporates: with no CUDA compiled in, the seven binaries are small and their
duplication stops mattering.
