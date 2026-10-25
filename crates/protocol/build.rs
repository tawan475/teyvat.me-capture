// SPDX-License-Identifier: MIT
// Copyright (c) 2024 IceDynamix and auto-artifactarium contributors
// Changes copyright (c) 2026 tawan475
// See THIRD_PARTY_NOTICES.md.

fn main() {
    protobuf_codegen::Codegen::new()
        .pure()
        .cargo_out_dir("protos")
        .include("protos")
        .inputs(["protos/protos.proto"])
        .run_from_script();

    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=protos/protos.proto");
}
