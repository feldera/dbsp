# The base image contains tools to build the code given that
# we need a Java and Rust compiler to run alongside the pipeline manager
# as of now. This will change later.
FROM ubuntu:22.04 AS base
ENV DEBIAN_FRONTEND noninteractive
RUN apt update && apt install \
     # pkg-config is required for cargo to find libssl
     libssl-dev pkg-config \
     # rdkafka dependency needs cmake and a CXX compiler
     cmake build-essential \
     # To install rust
     curl  \
     # For running the SQL compiler
     openjdk-19-jre-headless -y
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
# web-ui build tools
RUN curl -fsSL https://deb.nodesource.com/setup_19.x | bash -
RUN apt-get install --yes nodejs
RUN npm install --global yarn
RUN npm install --global openapi-typescript-codegen

# Use cargo-chef to produce a recipe.json file
# to cache the requisite dependencies
FROM base as chef
RUN /root/.cargo/bin/cargo install cargo-chef
RUN /root/.cargo/bin/cargo install cargo-make
WORKDIR app

# Cache dependencies from rust
FROM chef AS planner
COPY . .
RUN /root/.cargo/bin/cargo chef prepare --recipe-path recipe.json

# Use the recipe.json file to build dependencies first and cache that
# layer for faster incremental builds of source-code only changes
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN /root/.cargo/bin/cargo chef cook --release --recipe-path recipe.json --bin=dbsp_pipeline_manager --no-default-features
COPY . .
RUN /root/.cargo/bin/cargo build --release --bin=dbsp_pipeline_manager --no-default-features

# Java build can be performed in parallel
FROM base as javabuild
RUN apt install maven -y
RUN mkdir sql
COPY sql-to-dbsp-compiler /sql/sql-to-dbsp-compiler
RUN cd /sql/sql-to-dbsp-compiler/SQL-compiler && mvn -DskipTests package

# Minimal image for running the pipeline manager
FROM base as release
ENV PATH="$PATH:/root/.cargo/bin"
# Pipeline manager binary
COPY --from=builder /app/target/release/dbsp_pipeline_manager dbsp_pipeline_manager
# SQL compiler uber jar
RUN mkdir -p database-stream-processor/sql-to-dbsp-compiler/SQL-compiler/target
COPY --from=javabuild /sql/sql-to-dbsp-compiler/SQL-compiler/target/sql2dbsp-jar-with-dependencies.jar database-stream-processor/sql-to-dbsp-compiler/SQL-compiler/target/sql2dbsp-jar-with-dependencies.jar
# The crates needed for the SQL compiler
COPY crates/dbsp database-stream-processor/crates/dbsp
COPY crates/adapters database-stream-processor/crates/adapters
COPY crates/dataflow-jit database-stream-processor/crates/dataflow-jit
COPY README.md database-stream-processor/README.md
RUN mkdir -p database-stream-processor/sql-to-dbsp-compiler/lib

# Copy over the rust code and sql-to-dbsp script
COPY sql-to-dbsp-compiler/lib database-stream-processor/sql-to-dbsp-compiler/lib
COPY sql-to-dbsp-compiler/temp database-stream-processor/sql-to-dbsp-compiler/temp
COPY sql-to-dbsp-compiler/SQL-compiler/sql-to-dbsp database-stream-processor/sql-to-dbsp-compiler/SQL-compiler/sql-to-dbsp
# Run the precompile phase to speed up Rust compilations during deployment
RUN ./dbsp_pipeline_manager --bind-address=0.0.0.0 --manager-working-directory=/working-dir --compiler-working-directory=/working-dir --sql-compiler-home=/database-stream-processor/sql-to-dbsp-compiler --dbsp-override-path=/database-stream-processor --precompile
ENTRYPOINT ["./dbsp_pipeline_manager", "--bind-address=0.0.0.0", "--manager-working-directory=/working-dir", "--compiler-working-directory=/working-dir", "--sql-compiler-home=/database-stream-processor/sql-to-dbsp-compiler", "--dbsp-override-path=/database-stream-processor"]

##### The stages below are used to build the demo container

# Prepare SecOps simulator recipe
FROM chef as demo-planner
COPY ./demo/project_demo00-SecOps/simulator/ .
RUN /root/.cargo/bin/cargo chef prepare --recipe-path recipe.json

# Use the recipe.json file to build dependencies first and cache that
# layer for faster incremental builds of source-code only changes
FROM chef AS demo-builder
COPY --from=demo-planner /app/recipe.json recipe.json
RUN /root/.cargo/bin/cargo chef cook --release --recipe-path recipe.json
COPY ./demo/project_demo00-SecOps/simulator/ .
RUN /root/.cargo/bin/cargo build --release

# The dev target adds an rpk client and demo projects
FROM ubuntu:22.04 AS client
ENV DEBIAN_FRONTEND noninteractive
ENV PATH="$PATH:/root/.cargo/bin"
COPY --from=builder /app/target/release/dbsp_pipeline_manager dbsp_pipeline_manager
COPY demo demo
# Remove the simulator cargo project and the corresponding build artifacts
RUN rm -rf ./demo/project_demo00-SecOps/simulator/*
COPY --from=demo-builder /app/target/release/secops_simulator demo/project_demo00-SecOps/simulator/
COPY python python
RUN apt update && apt install pkg-config \
                              python3-pip python3-plumbum \
                              curl unzip -y --no-install-recommends \
    # Install RPK
    && arch=`dpkg --print-architecture`; \
      curl -LO https://github.com/redpanda-data/redpanda/releases/latest/download/rpk-linux-$arch.zip \
      && unzip rpk-linux-$arch.zip -d /bin/ \
      && rpk version \
      && rm rpk-linux-$arch.zip \
    # Install DBSP python library and dependencies
    && ./dbsp_pipeline_manager --dump-openapi \
      && rm dbsp_pipeline_manager \
      && pip3 install openapi-python-client websockets \
      && cd python \
      && rm -rf dbsp-api-client \
      && openapi-python-client generate --path ../openapi.json --fail-on-warning \
      && pip3 install ./dbsp-api-client \
      && pip3 install . \
   # TODO: only required for running the fraud detection demo. Remove when we clean that up.
   && pip3 install gdown \
   # cleanup packages we don't need anymore
   && apt remove python3-pip unzip pkg-config -y && apt autoremove -y
CMD bash

# By default, only build the release version
FROM release
