# Distribution example

DBSP performs computations using a collection of worker threads that
all implement the same circuit.  Worker threads interact only by
exchanging data at key points in the computation.  Distributed DBSP
extends this model by allowing exchange to occur across a network
rather than only through data structures within a single process.

This directory contains two programs that jointly demonstrate the
concept.  The first program, called `pool`, implements the same
circuit as the `tutorial9` example.  When it starts, it listens for
RPC connections on a TCP port specified on the command line.

The second program, called `coord`, accepts a list of `pool` addresses
to connect to, as well as a second list of addresses the `pool`
instances can use to exchange data among themselves.  It feeds data to
each `pool` in turn, steps the circuit, and prints the output.

For a demo with two "host"s, run each of these in turn in a separate
terminal.  If it works, the `coord` process will print the same output
that `tutorial9` would:

```
cargo run --example pool -- --address 127.0.0.1:1234
cargo run --example pool -- --address 127.0.0.1:1235
cargo run --example coord -- --pool 127.0.0.1:1234 --pool 127.0.0.1:1235 --exchange 127.0.0.1:1236 --exchange 127.0.0.1:1237 
```

