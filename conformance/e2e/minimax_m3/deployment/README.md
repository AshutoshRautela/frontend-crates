# Validation deployment

The experiment uses four NVIDIA B200 GPUs in one task-owned Kubernetes pod. The frontend and SGLang worker share file discovery inside the pod; no NATS or etcd service is needed. The model snapshot is `c5454eb03678d8710e54a4e0fc681b9f3b4a3dba` of `MiniMaxAI/MiniMax-M3-MXFP8` (451,543,283,200 weight bytes).

The manifests pin the runtime images. Supply an owned development namespace explicitly with both `kubectl --context "$M3_CONTEXT" --namespace "$M3_NAMESPACE" ...` options. The replay build pod omits the experiment's unused read-only shared-cache mount. Its `/work` and the model pod's `/weights` are temporary `emptyDir` volumes.

The build overlay uses Dynamo commit `178cc2f3f7` and the frontend-crates source from this branch. Copy their source archives to the build pod as `/tmp/dynamo-source.tar.gz` and `/tmp/frontend-source.tar.gz`, together with the three patch files, then run `build.sh`. The integration patch selects the local frontend crates and adds M3's prefill markers to Dynamo. The schema fallback patch selects JSON guidance when native M3 XML cannot preserve the requested schema. The separate compatibility patch removes a constructor argument unsupported by the pinned M3 development image; it does not change SGLang's parser.

Package `/work/overlay` into an archive and extract it into a fresh directory in the model pod. Keep the initial worker/v1 overlay at `/weights/overlay`; set `M3_FRONTEND_V2_OVERLAY` to a separate directory for each tested v2 build. This permits replacing the v2 frontend without changing the library mapped by the GPU worker.

- `start-sglang.sh INTERVAL` starts the native baseline on port 8000.
- `start-dynamo.sh INTERVAL true|false` starts a worker plus v1 on 8001 and v2 on 8000. The final argument controls structural tags.
- `stop-servers.sh` stops only the experiment's recorded process groups and clears its temporary discovery records. Wait for a nonempty `/v1/models` before testing the next Dynamo phase.

The native baseline exports the same production v2 structural constraint with an external reasoning boundary. Deployment package versions and binary hashes accompany each captured run.

Delete both task-created pods when testing finishes, and verify their deletion. Preserve pre-existing namespaces, RoleBindings and shared PVCs.
