VERSION 0.7
FROM ubuntu:22.04

RUN apt-get update && apt-get install --yes sudo

WORKDIR /dbsp
ENV RUSTUP_HOME=$HOME/.rustup
ENV CARGO_HOME=$HOME/.cargo
# Adds python and rust binaries to thep path
ENV PATH=$HOME/.cargo/bin:$HOME/.local/bin:$PATH
ENV RUST_VERSION=1.70.0
ENV RUST_BUILD_MODE='' # set to --release for release builds

install-deps:
    RUN apt-get update
    RUN apt-get install --yes build-essential curl libssl-dev build-essential pkg-config \
                              cmake git gcc clang libclang-dev python3-pip python3-plumbum \
                              hub numactl openjdk-19-jre-headless maven netcat jq \
                              libsasl2-dev docker.io libenchant-2-2 graphviz
    RUN curl -fsSL https://deb.nodesource.com/setup_19.x | sudo -E bash - && apt-get install -y nodejs
    RUN npm install --global yarn
    RUN npm install --global openapi-typescript-codegen

install-rust:
    FROM +install-deps
    RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- \
        -y \
        --default-toolchain $RUST_VERSION \
        --profile minimal \
        --component clippy \
        --component rustfmt \
        --component llvm-tools-preview
    RUN rustup toolchain install nightly \
        --component clippy \
        --component rustfmt \
        --component llvm-tools-preview
    RUN cargo install --locked --force --version 0.5.0 cargo-machete
    RUN cargo install --locked --force --version 0.17.6 cargo-audit
    RUN cargo install --locked --force --version 0.36.11 cargo-make
    RUN cargo install --locked --force --version 0.5.22 cargo-llvm-cov
    RUN cargo install --locked --force --version 0.1.61 cargo-chef
    RUN rustup --version
    RUN cargo --version
    RUN rustc --version

install-python-deps:
    FROM +install-deps
    RUN pip install wheel
    COPY demo/demo_notebooks/requirements.txt requirements.txt
    RUN pip wheel -r requirements.txt --wheel-dir=wheels
    SAVE ARTIFACT wheels /wheels

install-python:
    FROM +install-deps
    COPY +install-python-deps/wheels wheels
    COPY demo/demo_notebooks/requirements.txt requirements.txt
    RUN pip install --user -v --no-index --find-links=wheels -r requirements.txt
    COPY python python
    RUN cd python && pip3 install --user -v ./dbsp-api-client
    RUN cd python && pip3 install --user -v .
    SAVE ARTIFACT /root/.local/lib/python3.10
    SAVE ARTIFACT /root/.local/bin

build-webui-deps:
    FROM +install-deps
    COPY web-ui/package.json ./web-ui/package.json
    COPY web-ui/yarn.lock ./web-ui/yarn.lock

    RUN cd web-ui && yarn install

build-webui:
    FROM +build-webui-deps
    COPY --dir web-ui/public web-ui/public
    COPY --dir web-ui/src web-ui/src
    COPY --dir web-ui/styles web-ui/styles
    COPY web-ui/.editorconfig web-ui/
    COPY web-ui/.eslintrc.json web-ui/
    COPY web-ui/.prettierrc.js web-ui/
    COPY web-ui/next-env.d.ts ./web-ui/next-env.d.ts
    COPY web-ui/next.config.js ./web-ui/next.config.js
    COPY web-ui/next.d.ts ./web-ui/next.d.ts
    COPY web-ui/tsconfig.json ./web-ui/tsconfig.json

    RUN cd web-ui && yarn format:check
    RUN cd web-ui && yarn build
    RUN cd web-ui && yarn export
    SAVE ARTIFACT ./web-ui/out

prepare-cache:
    # We download and pre-build dependencies to cache it using cargo-chef.
    # See also (on why this is so complicated :/):
    # https://github.com/rust-lang/cargo/issues/2644
    # https://hackmd.io/@kobzol/S17NS71bh
    FROM +install-rust

    # We can't just copy crates from source, the reasons seem to have to do with
    # the way the hashes are computed for the cache: e.g., once
    # https://github.com/earthly/earthly/issues/786 is fixed this can go away
    # and crates can be copied instead:
    RUN mkdir -p .cargo
    RUN mkdir -p crates/dataflow-jit
    RUN mkdir -p crates/nexmark
    RUN mkdir -p crates/dbsp
    RUN mkdir -p crates/adapters
    RUN mkdir -p crates/pipeline_manager
    RUN mkdir -p sql-to-dbsp-compiler/lib/hashing
    RUN mkdir -p sql-to-dbsp-compiler/lib/readers
    RUN mkdir -p sql-to-dbsp-compiler/lib/sqllib
    RUN mkdir -p sql-to-dbsp-compiler/lib/sqlvalue
    RUN mkdir -p sql-to-dbsp-compiler/lib/tuple
    RUN mkdir -p sql-to-dbsp-compiler/temp
    #RUN mkdir -p crates/webui-tester

    COPY --keep-ts .cargo/config .cargo/config
    COPY --keep-ts Cargo.toml .
    COPY --keep-ts Cargo.lock .
    COPY --keep-ts crates/dataflow-jit/Cargo.toml crates/dataflow-jit/
    COPY --keep-ts crates/nexmark/Cargo.toml crates/nexmark/
    COPY --keep-ts crates/dbsp/Cargo.toml crates/dbsp/
    COPY --keep-ts crates/adapters/Cargo.toml crates/adapters/
    COPY --keep-ts crates/pipeline_manager/Cargo.toml crates/pipeline_manager/
    #COPY --keep-ts crates/webui-tester/Cargo.toml crates/webui-tester/
    COPY --keep-ts sql-to-dbsp-compiler/lib/hashing/Cargo.toml sql-to-dbsp-compiler/lib/hashing/
    COPY --keep-ts sql-to-dbsp-compiler/lib/readers/Cargo.toml sql-to-dbsp-compiler/lib/readers/
    COPY --keep-ts sql-to-dbsp-compiler/lib/sqllib/Cargo.toml sql-to-dbsp-compiler/lib/sqllib/
    COPY --keep-ts sql-to-dbsp-compiler/lib/sqlvalue/Cargo.toml sql-to-dbsp-compiler/lib/sqlvalue/
    COPY --keep-ts sql-to-dbsp-compiler/lib/tuple/Cargo.toml sql-to-dbsp-compiler/lib/tuple/
    COPY --keep-ts sql-to-dbsp-compiler/temp/Cargo.toml sql-to-dbsp-compiler/temp/

    RUN mkdir -p crates/dataflow-jit/src && touch crates/dataflow-jit/src/lib.rs
    RUN mkdir -p crates/nexmark/src && touch crates/nexmark/src/lib.rs
    RUN mkdir -p crates/dbsp/src && touch crates/dbsp/src/lib.rs
    RUN mkdir -p crates/dbsp/examples && touch crates/dbsp/examples/degrees.rs && touch crates/dbsp/examples/orgchart.rs
    RUN mkdir -p crates/adapters/src && touch crates/adapters/src/lib.rs
    RUN mkdir -p crates/adapters/examples && touch crates/adapters/examples/server.rs
    RUN mkdir -p crates/dataflow-jit/src && touch crates/dataflow-jit/src/main.rs
    RUN mkdir -p crates/nexmark/benches/nexmark-gen && touch crates/nexmark/benches/nexmark-gen/main.rs
    RUN mkdir -p crates/nexmark/benches/nexmark && touch crates/nexmark/benches/nexmark/main.rs
    RUN mkdir -p crates/dbsp/benches/gdelt && touch crates/dbsp/benches/gdelt/main.rs
    RUN mkdir -p crates/dbsp/benches/ldbc-graphalytics && touch crates/dbsp/benches/ldbc-graphalytics/main.rs
    RUN mkdir -p crates/dbsp/benches
    RUN touch crates/dbsp/benches/galen.rs
    RUN touch crates/dbsp/benches/fraud.rs
    RUN touch crates/dbsp/benches/path.rs
    RUN touch crates/dbsp/benches/consolidation.rs
    RUN touch crates/dbsp/benches/column_layer.rs
    RUN mkdir -p crates/pipeline_manager/src && touch crates/pipeline_manager/src/main.rs
    #RUN mkdir -p crates/webui-tester/src && touch crates/webui-tester/src/lib.rs
    RUN mkdir -p sql-to-dbsp-compiler/lib/hashing/src && touch sql-to-dbsp-compiler/lib/hashing/src/lib.rs
    RUN mkdir -p sql-to-dbsp-compiler/lib/readers/src && touch sql-to-dbsp-compiler/lib/readers/src/lib.rs
    RUN mkdir -p sql-to-dbsp-compiler/lib/sqllib/src && touch sql-to-dbsp-compiler/lib/sqllib/src/lib.rs
    RUN mkdir -p sql-to-dbsp-compiler/lib/sqlvalue/src && touch sql-to-dbsp-compiler/lib/sqlvalue/src/lib.rs
    RUN mkdir -p sql-to-dbsp-compiler/lib/tuple/src && touch sql-to-dbsp-compiler/lib/tuple/src/lib.rs

    ENV RUST_LOG=info
    RUN cargo chef prepare

    SAVE ARTIFACT --keep-ts recipe.json

build-cache:
    ARG RUST_TOOLCHAIN=$RUST_VERSION
    ARG RUST_BUILD_PROFILE=$RUST_BUILD_MODE

    FROM +install-rust
    COPY --keep-ts +prepare-cache/recipe.json ./
    RUN cargo +$RUST_TOOLCHAIN chef cook $RUST_BUILD_PROFILE --workspace --all-targets
    RUN cargo +$RUST_TOOLCHAIN chef cook $RUST_BUILD_PROFILE --clippy --workspace
    # I have no clue why we need all these commands below to build all
    # dependencies (since we just did a build --workspace --all-targets). But if
    # we don't run all of them, it will go and compile dependencies during
    # the `build-*` targets (which should just compile our crates ideally to
    # maximize the cache benefit). This happens even without earthly (or docker)
    # if you just run cargo in our repo directly on clean target...
    #
    # The only issues for this I found was:
    # https://github.com/rust-lang/cargo/issues/2904
    # https://github.com/rust-lang/cargo/issues/6733
    #
    # When using `RUST_LOG=cargo::ops::cargo_rustc::fingerprint=info` to debug,
    # it looks like the hash for the crates changes.
    RUN cargo +$RUST_TOOLCHAIN build $RUST_BUILD_PROFILE
    RUN cargo +$RUST_TOOLCHAIN test $RUST_BUILD_PROFILE --no-run
    RUN cargo +$RUST_TOOLCHAIN clippy $RUST_BUILD_PROFILE
    RUN cargo +$RUST_TOOLCHAIN build $RUST_BUILD_PROFILE --package dbsp
    RUN cargo +$RUST_TOOLCHAIN test $RUST_BUILD_PROFILE --package dbsp --no-run
    RUN cargo +$RUST_TOOLCHAIN test $RUST_BUILD_PROFILE --package dbsp --features persistence --no-run
    RUN cargo +$RUST_TOOLCHAIN clippy $RUST_BUILD_PROFILE --package dbsp
    RUN cargo +$RUST_TOOLCHAIN build $RUST_BUILD_PROFILE --package dbsp_adapters
    RUN cargo +$RUST_TOOLCHAIN test $RUST_BUILD_PROFILE --package dbsp_adapters --no-run
    RUN cargo +$RUST_TOOLCHAIN clippy $RUST_BUILD_PROFILE --package dbsp_adapters
    RUN cargo +$RUST_TOOLCHAIN build $RUST_BUILD_PROFILE --package dbsp_pipeline_manager
    RUN cargo +$RUST_TOOLCHAIN test $RUST_BUILD_PROFILE --package dbsp_pipeline_manager --no-run
    RUN cargo +$RUST_TOOLCHAIN clippy $RUST_BUILD_PROFILE --package dbsp_pipeline_manager
    RUN cargo +$RUST_TOOLCHAIN build $RUST_BUILD_PROFILE --package dataflow-jit
    RUN cargo +$RUST_TOOLCHAIN test $RUST_BUILD_PROFILE --package dataflow-jit --no-run
    RUN cargo +$RUST_TOOLCHAIN clippy $RUST_BUILD_PROFILE --package dataflow-jit
    RUN cargo +$RUST_TOOLCHAIN build $RUST_BUILD_PROFILE --package dbsp_nexmark
    RUN cargo +$RUST_TOOLCHAIN test $RUST_BUILD_PROFILE --package dbsp_nexmark --no-run
    RUN cargo +$RUST_TOOLCHAIN clippy $RUST_BUILD_PROFILE --package dbsp_nexmark
    RUN cargo +$RUST_TOOLCHAIN build $RUST_BUILD_PROFILE --package hashing
    RUN cargo +$RUST_TOOLCHAIN test $RUST_BUILD_PROFILE --package hashing --no-run
    RUN cargo +$RUST_TOOLCHAIN build $RUST_BUILD_PROFILE --package readers
    RUN cargo +$RUST_TOOLCHAIN test $RUST_BUILD_PROFILE --package readers --no-run
    RUN cargo +$RUST_TOOLCHAIN build $RUST_BUILD_PROFILE --package sqllib
    RUN cargo +$RUST_TOOLCHAIN test $RUST_BUILD_PROFILE --package sqllib --no-run
    RUN cargo +$RUST_TOOLCHAIN build $RUST_BUILD_PROFILE --package sqlvalue
    RUN cargo +$RUST_TOOLCHAIN test $RUST_BUILD_PROFILE --package sqlvalue --no-run
    RUN cargo +$RUST_TOOLCHAIN build $RUST_BUILD_PROFILE --package tuple
    RUN cargo +$RUST_TOOLCHAIN test $RUST_BUILD_PROFILE --package tuple --no-run

    # If we make this a workspace crate we can use the chef/cook commands but
    # it breaks `cargo build` in non-CI builds (because there is no
    # lib.rs and we don't want to check that in as it gets overwritten)
    COPY --dir sql-to-dbsp-compiler/temp sql-to-dbsp-compiler/temp
    RUN rm -f sql-to-dbsp-compiler/temp/src/lib.rs && touch sql-to-dbsp-compiler/temp/src/lib.rs
    RUN cd sql-to-dbsp-compiler/temp && cargo +$RUST_TOOLCHAIN test $RUST_BUILD_PROFILE --no-run

    SAVE ARTIFACT --keep-ts $CARGO_HOME
    SAVE ARTIFACT --keep-ts ./target

build-dbsp:
    ARG RUST_TOOLCHAIN=$RUST_VERSION
    ARG RUST_BUILD_PROFILE=$RUST_BUILD_MODE

    FROM +build-cache --RUST_TOOLCHAIN=$RUST_TOOLCHAIN --RUST_BUILD_PROFILE=$RUST_BUILD_PROFILE

    # Remove the skeleton created in build-cache
    RUN rm -rf crates/dbsp
    # Copy in the actual sources
    COPY --keep-ts --dir crates/dbsp crates/dbsp
    COPY --keep-ts README.md README.md

    RUN cargo +$RUST_TOOLCHAIN build $RUST_BUILD_PROFILE --package dbsp
    RUN cd crates/dbsp && cargo +$RUST_TOOLCHAIN machete
    RUN cargo +$RUST_TOOLCHAIN clippy $RUST_BUILD_PROFILE --package dbsp -- -D warnings
    RUN cargo +$RUST_TOOLCHAIN test $RUST_BUILD_PROFILE --package dbsp --no-run
    # Update target folder for future tasks, the whole --keep-ts,
    # args were an attempt in fixing the dependency problem (see build-cache)
    # but it doesn't seem to make a difference.
    SAVE ARTIFACT --keep-ts ./target/* ./target

build-adapters:
    ARG RUST_TOOLCHAIN=$RUST_VERSION
    ARG RUST_BUILD_PROFILE=$RUST_BUILD_MODE

    FROM +build-dbsp --RUST_TOOLCHAIN=$RUST_TOOLCHAIN --RUST_BUILD_PROFILE=$RUST_BUILD_PROFILE
    RUN rm -rf crates/adapters
    COPY --keep-ts --dir crates/adapters crates/adapters

    RUN cargo +$RUST_TOOLCHAIN build $RUST_BUILD_PROFILE --package dbsp_adapters
    RUN cd crates/adapters && cargo +$RUST_TOOLCHAIN machete
    RUN cargo +$RUST_TOOLCHAIN clippy $RUST_BUILD_PROFILE --package dbsp_adapters -- -D warnings
    RUN cargo +$RUST_TOOLCHAIN test $RUST_BUILD_PROFILE --package dbsp_adapters --no-run

    SAVE ARTIFACT --keep-ts ./target/* ./target

build-dataflow-jit:
    ARG RUST_TOOLCHAIN=$RUST_VERSION
    ARG RUST_BUILD_PROFILE=$RUST_BUILD_MODE

    FROM +build-adapters --RUST_TOOLCHAIN=$RUST_TOOLCHAIN --RUST_BUILD_PROFILE=$RUST_BUILD_PROFILE

    RUN rm -rf crates/dataflow-jit
    COPY --keep-ts --dir crates/dataflow-jit crates/dataflow-jit

    RUN cargo +$RUST_TOOLCHAIN build $RUST_BUILD_PROFILE --package dataflow-jit
    RUN cd crates/dataflow-jit && cargo +$RUST_TOOLCHAIN machete
    RUN cargo +$RUST_TOOLCHAIN test $RUST_BUILD_PROFILE --package dataflow-jit --no-run
    RUN cargo +$RUST_TOOLCHAIN clippy $RUST_BUILD_PROFILE --package dataflow-jit -- -D warnings
    SAVE ARTIFACT --keep-ts ./target/* ./target

build-manager:
    ARG RUST_TOOLCHAIN=$RUST_VERSION
    ARG RUST_BUILD_PROFILE=$RUST_BUILD_MODE

    FROM +build-dataflow-jit --RUST_TOOLCHAIN=$RUST_TOOLCHAIN --RUST_BUILD_PROFILE=$RUST_BUILD_PROFILE
    # For some reason if this ENV before the FROM line it gets invalidated
    ENV WEBUI_BUILD_DIR=/dbsp/web-ui/out
    COPY ( +build-webui/out ) ./web-ui/out

    RUN rm -rf crates/pipeline_manager
    COPY --keep-ts --dir crates/pipeline_manager crates/pipeline_manager

    RUN cargo +$RUST_TOOLCHAIN build $RUST_BUILD_PROFILE --package dbsp_pipeline_manager
    RUN cd crates/pipeline_manager && cargo +$RUST_TOOLCHAIN machete
    RUN cargo +$RUST_TOOLCHAIN clippy $RUST_BUILD_PROFILE --package dbsp_pipeline_manager -- -D warnings
    RUN cargo +$RUST_TOOLCHAIN test $RUST_BUILD_PROFILE --package dbsp_pipeline_manager --no-run

    IF [ -f ./target/debug/dbsp_pipeline_manager ]
        SAVE ARTIFACT --keep-ts ./target/debug/dbsp_pipeline_manager dbsp_pipeline_manager
    END
    IF [ -f ./target/release/dbsp_pipeline_manager ]
        SAVE ARTIFACT --keep-ts ./target/release/dbsp_pipeline_manager dbsp_pipeline_manager
    END

test-sql:
    ARG RUST_TOOLCHAIN=$RUST_VERSION
    ARG RUST_BUILD_PROFILE=$RUST_BUILD_MODE

    FROM +build-dataflow-jit --RUST_TOOLCHAIN=$RUST_TOOLCHAIN --RUST_BUILD_PROFILE=$RUST_BUILD_PROFILE
    COPY --keep-ts sql-to-dbsp-compiler sql-to-dbsp-compiler

    COPY demo/hello-world/combiner.sql demo/hello-world/combiner.sql
    COPY demo/project_demo00-SecOps/project.sql demo/project_demo00-SecOps/project.sql
    COPY demo/project_demo01-TimeSeriesEnrich/project.sql demo/project_demo01-TimeSeriesEnrich/project.sql
    COPY demo/project_demo02-FraudDetection/project.sql demo/project_demo02-FraudDetection/project.sql
    COPY demo/project_demo03-GreenTrip/project.sql demo/project_demo03-GreenTrip/project.sql
    COPY demo/project_demo04-SimpleSelect/project.sql demo/project_demo04-SimpleSelect/project.sql

    CACHE /root/.m2

    COPY sql-to-dbsp-compiler sql-to-dbsp-compiler
    RUN cd "sql-to-dbsp-compiler/SQL-compiler" && mvn package
    SAVE ARTIFACT sql-to-dbsp-compiler/SQL-compiler/target/sql2dbsp-jar-with-dependencies.jar sql2dbsp-jar-with-dependencies.jar
    SAVE ARTIFACT sql-to-dbsp-compiler

install-docs-deps:
    FROM +install-deps
    COPY docs/package.json ./docs/package.json
    COPY docs/yarn.lock ./docs/yarn.lock
    RUN cd docs && yarn install

build-docs:
    FROM +install-docs-deps
    COPY docs/ docs/
    COPY ( +build-manager/dbsp_pipeline_manager ) ./docs/dbsp_pipeline_manager
    RUN cd docs && ./dbsp_pipeline_manager --dump-openapi
    RUN cd docs && yarn format:check
    RUN cd docs && yarn lint
    RUN cd docs && yarn build --no-minify
    SAVE ARTIFACT docs/out AS LOCAL docs/out

build-nexmark:
    ARG RUST_TOOLCHAIN=$RUST_VERSION
    ARG RUST_BUILD_PROFILE=$RUST_BUILD_MODE

    FROM +build-dbsp --RUST_TOOLCHAIN=$RUST_TOOLCHAIN --RUST_BUILD_PROFILE=$RUST_BUILD_PROFILE

    RUN rm -rf crates/nexmark
    COPY --keep-ts --dir crates/nexmark crates/nexmark

    RUN cargo +$RUST_TOOLCHAIN build $RUST_BUILD_PROFILE --package dbsp_nexmark
    RUN cd crates/nexmark && cargo +$RUST_TOOLCHAIN machete
    RUN cargo +$RUST_TOOLCHAIN test $RUST_BUILD_PROFILE --package dbsp_nexmark --no-run
    RUN cargo +$RUST_TOOLCHAIN clippy $RUST_BUILD_PROFILE --package dbsp_nexmark -- -D warnings

audit:
    FROM +build-cache
    RUN cargo audit || true

CARGO_TEST:
    COMMAND
    ARG RUST_TOOLCHAIN=$RUST_VERSION
    ARG RUST_BUILD_PROFILE=$RUST_BUILD_MODE
    ARG package
    ARG features
    ARG test_args
    RUN cargo +$RUST_TOOLCHAIN test $RUST_BUILD_PROFILE --package $package \
        $(if [ -z $features ]; then printf -- --features $features; fi) \
        -- $test_args

test-dbsp:
    ARG RUST_TOOLCHAIN=$RUST_VERSION
    ARG RUST_BUILD_PROFILE=$RUST_BUILD_MODE

    FROM +build-dbsp --RUST_TOOLCHAIN=$RUST_TOOLCHAIN --RUST_BUILD_PROFILE=$RUST_BUILD_PROFILE
    # Limit test execution to tests in trace::persistent::tests, because
    # executing everything takes too long and (in theory) the proptests we have
    # should ensure equivalence with the DRAM trace implementation:
    DO +CARGO_TEST \
        --RUST_TOOLCHAIN=$RUST_TOOLCHAIN --RUST_BUILD_PROFILE=$RUST_BUILD_PROFILE \
        --package=dbsp --features=persistence --test_args=trace::persistent::tests
    RUN cargo +$RUST_TOOLCHAIN test $RUST_BUILD_PROFILE --package dbsp

test-nexmark:
    ARG RUST_TOOLCHAIN=$RUST_VERSION
    ARG RUST_BUILD_PROFILE=$RUST_BUILD_MODE

    FROM +build-nexmark --RUST_TOOLCHAIN=$RUST_TOOLCHAIN --RUST_BUILD_PROFILE=$RUST_BUILD_PROFILE
    RUN cargo +$RUST_TOOLCHAIN test $RUST_BUILD_PROFILE --package dbsp_nexmark

test-dataflow-jit:
    ARG RUST_TOOLCHAIN=$RUST_VERSION
    ARG RUST_BUILD_PROFILE=$RUST_BUILD_MODE

    FROM +build-dataflow-jit --RUST_TOOLCHAIN=$RUST_TOOLCHAIN --RUST_BUILD_PROFILE=$RUST_BUILD_PROFILE
    # Tests use this demo directory
    COPY --keep-ts demo/project_demo01-TimeSeriesEnrich demo/project_demo01-TimeSeriesEnrich
    RUN cargo +$RUST_TOOLCHAIN test $RUST_BUILD_PROFILE --package dataflow-jit

test-adapters:
    ARG RUST_TOOLCHAIN=$RUST_VERSION
    ARG RUST_BUILD_PROFILE=$RUST_BUILD_MODE

    FROM +build-adapters --RUST_TOOLCHAIN=$RUST_TOOLCHAIN --RUST_BUILD_PROFILE=$RUST_BUILD_PROFILE
    WITH DOCKER --pull docker.redpanda.com/vectorized/redpanda:v23.1.13
        RUN docker run -p 9092:9092 --rm -itd docker.redpanda.com/vectorized/redpanda:v23.1.13 \
            redpanda start --smp 2 && \
            # Redpanda takes a few seconds to initialize.
            sleep 10 && \
            cargo +$RUST_TOOLCHAIN test $RUST_BUILD_PROFILE --package dbsp_adapters
    END

test-manager:
    ARG RUST_TOOLCHAIN=$RUST_VERSION
    ARG RUST_BUILD_PROFILE=$RUST_BUILD_MODE

    FROM +build-manager --RUST_TOOLCHAIN=$RUST_TOOLCHAIN --RUST_BUILD_PROFILE=$RUST_BUILD_PROFILE
    ENV PGHOST=localhost
    ENV PGUSER=postgres
    ENV PGCLIENTENCODING=UTF8
    ENV PGPORT=5432
    ENV RUST_LOG=error
    WITH DOCKER --pull postgres
        # We just put the PGDATA in /dev/shm because the docker fs seems very slow (test time goes to 2min vs. shm 40s)
        RUN docker run --shm-size=512MB -p 5432:5432 -e POSTGRES_HOST_AUTH_METHOD=trust -e PGDATA=/dev/shm -d postgres && \
            # Sleep until postgres is up (otherwise we get connection reset if we connect too early)
            # (See: https://github.com/docker-library/docs/blob/master/postgres/README.md#caveats)
            sleep 3 && \
            cargo +$RUST_TOOLCHAIN test $RUST_BUILD_PROFILE --package dbsp_pipeline_manager
    END
    # We keep the test binary around so we can run integration tests later. This incantation is used to find the
    # test binary path, adapted from: https://github.com/rust-lang/cargo/issues/3670
    RUN cp `cargo test --features integration-test --no-run --package dbsp_pipeline_manager --message-format=json | jq -r 'select(.target.kind[0] == "bin") | .executable' | grep -v null` test_binary
    SAVE ARTIFACT test_binary

python-bindings-checker:
    ARG RUST_TOOLCHAIN=$RUST_VERSION
    ARG RUST_BUILD_PROFILE=$RUST_BUILD_MODE

    FROM +build-manager --RUST_TOOLCHAIN=$RUST_TOOLCHAIN --RUST_BUILD_PROFILE=$RUST_BUILD_PROFILE
    COPY +build-manager/dbsp_pipeline_manager .
    RUN mkdir -p /root/.local/lib/python3.10
    RUN mkdir -p /root/.local/bin

    COPY +install-python/python3.10 /root/.local/lib/python3.10
    COPY +install-python/bin /root/.local/bin

    RUN pip3 install openapi-python-client
    COPY +build-manager/dbsp_pipeline_manager .
    COPY +test-sql/sql-to-dbsp-compiler sql-to-dbsp-compiler
    COPY python/dbsp-api-client dbsp-api-client-base

    # This line will fail if the pytdochon bindings need to be regenerated
    RUN mkdir checker
    RUN cd checker && ../dbsp_pipeline_manager --dump-openapi &&  \
        openapi-python-client generate --path openapi.json --fail-on-warning && \
        diff -bur dbsp-api-client ../dbsp-api-client-base


test-python:
    ARG RUST_TOOLCHAIN=$RUST_VERSION
    ARG RUST_BUILD_PROFILE=$RUST_BUILD_MODE

    FROM +build-manager --RUST_TOOLCHAIN=$RUST_TOOLCHAIN --RUST_BUILD_PROFILE=$RUST_BUILD_PROFILE
    COPY +build-manager/dbsp_pipeline_manager .
    RUN mkdir -p /root/.local/lib/python3.10
    RUN mkdir -p /root/.local/bin

    COPY +install-python/python3.10 /root/.local/lib/python3.10
    COPY +install-python/bin /root/.local/bin

    COPY +build-manager/dbsp_pipeline_manager .
    COPY +test-sql/sql-to-dbsp-compiler sql-to-dbsp-compiler

    COPY demo/demo_notebooks demo/demo_notebooks
    COPY python/test.py python/test.py

    ENV PGHOST=localhost
    ENV PGUSER=postgres
    ENV PGCLIENTENCODING=UTF8
    ENV PGPORT=5432
    ENV RUST_LOG=error
    ENV WITH_POSTGRES=1
    ENV IN_CI=1
    WITH DOCKER --pull postgres
        RUN docker run --shm-size=512MB -p 5432:5432 -e POSTGRES_HOST_AUTH_METHOD=trust -e PGDATA=/dev/shm -d postgres && \
            sleep 3 && \
            ./dbsp_pipeline_manager --bind-address=0.0.0.0 --manager-working-directory=/working-dir --compiler-working-directory=/working-dir --sql-compiler-home=/dbsp/sql-to-dbsp-compiler --dbsp-override-path=/dbsp --db-connection-string=postgresql://postgres:postgres@localhost:5432 --unix-daemon && \
            sleep 1 && \
            python3 python/test.py && \
            cd demo/demo_notebooks && jupyter execute fraud_detection.ipynb --JupyterApp.log_level='DEBUG'
    END

test-rust:
    ARG RUST_TOOLCHAIN=$RUST_VERSION
    ARG RUST_BUILD_PROFILE=$RUST_BUILD_MODE

    BUILD +test-dbsp --RUST_TOOLCHAIN=$RUST_TOOLCHAIN --RUST_BUILD_PROFILE=$RUST_BUILD_PROFILE
    BUILD +test-nexmark --RUST_TOOLCHAIN=$RUST_TOOLCHAIN --RUST_BUILD_PROFILE=$RUST_BUILD_PROFILE
    BUILD +test-adapters --RUST_TOOLCHAIN=$RUST_TOOLCHAIN --RUST_BUILD_PROFILE=$RUST_BUILD_PROFILE
    BUILD +test-dataflow-jit --RUST_TOOLCHAIN=$RUST_TOOLCHAIN --RUST_BUILD_PROFILE=$RUST_BUILD_PROFILE
    BUILD +test-manager --RUST_TOOLCHAIN=$RUST_TOOLCHAIN --RUST_BUILD_PROFILE=$RUST_BUILD_PROFILE

# TODO: the following two container tasks duplicate work that we otherwise do in the Dockerfile,
# but by mostly repeating ourselves, we can reuse earlier Earthly stages to speed up the CI.
build-dbsp-manager-container:
    FROM +install-rust
    WORKDIR /

    # First, copy over the artifacts built from previous stages
    RUN mkdir -p database-stream-processor/sql-to-dbsp-compiler/SQL-compiler/target
    COPY +build-manager/dbsp_pipeline_manager .
    COPY +test-sql/sql2dbsp-jar-with-dependencies.jar database-stream-processor/sql-to-dbsp-compiler/SQL-compiler/target/

    # Then copy over the crates needed by the sql compiler
    COPY crates/dbsp database-stream-processor/crates/dbsp
    COPY crates/adapters database-stream-processor/crates/adapters
    COPY crates/dataflow-jit database-stream-processor/crates/dataflow-jit
    COPY README.md database-stream-processor/README.md

    # Then copy over the required SQL compiler files
    COPY sql-to-dbsp-compiler/SQL-compiler/sql-to-dbsp /database-stream-processor/sql-to-dbsp-compiler/SQL-compiler/sql-to-dbsp
    COPY sql-to-dbsp-compiler/lib /database-stream-processor/sql-to-dbsp-compiler/lib
    COPY sql-to-dbsp-compiler/temp /database-stream-processor/sql-to-dbsp-compiler/temp
    RUN ./dbsp_pipeline_manager --bind-address=0.0.0.0 --manager-working-directory=/working-dir --compiler-working-directory=/working-dir --sql-compiler-home=/database-stream-processor/sql-to-dbsp-compiler --dbsp-override-path=/database-stream-processor --precompile
    ENTRYPOINT ["./dbsp_pipeline_manager", "--bind-address=0.0.0.0", "--manager-working-directory=/working-dir", "--compiler-working-directory=/working-dir", "--sql-compiler-home=/database-stream-processor/sql-to-dbsp-compiler", "--dbsp-override-path=/database-stream-processor"]
    SAVE IMAGE ghcr.io/feldera/dbsp-manager

# TODO: mirrors the Dockerfile. See note above.
build-demo-container:
    FROM +install-rust
    WORKDIR /
    RUN apt install unzip -y
    RUN arch=`dpkg --print-architecture`; \
            curl -LO https://github.com/redpanda-data/redpanda/releases/latest/download/rpk-linux-$arch.zip \
            && unzip rpk-linux-$arch.zip -d /bin/ \
            && rpk version \
            && rm rpk-linux-$arch.zip
    COPY +install-python/python3.10 /root/.local/lib/python3.10
    COPY +install-python/bin /root/.local/bin
    COPY demo demo
    CMD bash
    SAVE IMAGE ghcr.io/feldera/demo-container

test-docker-compose:
    FROM earthly/dind:alpine
    COPY deploy/docker-compose.yml .
    WITH DOCKER --pull postgres \
                --pull docker.redpanda.com/vectorized/redpanda:v23.1.13 \
                --load ghcr.io/feldera/dbsp-manager=+build-dbsp-manager-container \
                --load ghcr.io/feldera/demo-container=+build-demo-container
        RUN SECOPS_DEMO_ARGS="--prepare-args 500000" RUST_LOG=debug,tokio_postgres=info docker-compose -f docker-compose.yml --profile demo up --force-recreate --exit-code-from demo
    END

# Fetches the test binary from test-manager, and produces a container image out of it
integration-test-container:
    FROM +install-deps
    COPY +test-manager/test_binary .
    ENV TEST_DBSP_URL=http://dbsp:8080
    ENTRYPOINT ["./test_binary", "integration_test::"]
    SAVE IMAGE itest:latest

# Runs the integration test container against the docker compose setup
integration-tests:
    FROM earthly/dind:alpine
    COPY deploy/docker-compose.yml .
    COPY deploy/.env .
    WITH DOCKER --pull postgres \
                --load ghcr.io/feldera/dbsp-manager=+build-dbsp-manager-container \
                --compose docker-compose.yml \
                --service db \
                --service dbsp \
                --load itest:latest=+integration-test-container
        RUN sleep 5 && docker run --env-file .env --network default_default itest:latest
    END

all-tests:
    BUILD +test-rust
    BUILD +test-python
    BUILD +audit
    BUILD +python-bindings-checker
    BUILD +test-sql
    BUILD +test-docker-compose
    BUILD +integration-tests
