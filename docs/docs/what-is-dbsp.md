# What is DBSP?

DBSP, which stands for "DataBase Stream Processor," is a fast, open
source continuous query engine with a rich SQL feature set.  With DBSP,
sophisticated SQL written for popular databases can operate
continuously, with few changes, on data as it arrives in real time.
This site explains the concepts behind DBSP and how to use it through
its user interface and API.

## Concepts

DBSP processes queries and produces output continuously.  When input
arrives, DBSP recomputes query results and sends the changes to
outputs.  DBSP queries are written in SQL, so users who have an
existing investment in analyzing data at rest with a SQL database can
use much the same code to analyze data continuously with DBSP.

### Incremental processing

DBSP is fundamentally **incremental** in how it handles input,
computation, and output.

For input, being incremental means that DBSP processes data as it
arrives.  DBSP does not require all of the data to
be on hand before beginning computation.  Unlike a database, DBSP does
not durably store data.

For computation, being incremental means that when new data arrives,
DBSP does a minimal amount of work to update query results, rather
than by fully recomputing them.  This speeds up processing.

For output, being incremental means that DBSP outputs query results as
sets of changes from previous output.  This improves performance and
reduces output size.

### Programs

In DSBP, a SQL streaming **program** is a collection of SQL table and
view definitions:

* SQL table definitions with `CREATE TABLE` specify the format of
  data.  DBSP does not store tables, since it is not a database, so
  table definitions do not reserve disk space or other storage.

* SQL view definitions with `CREATE VIEW` specify transformations and
  computations.  Views may draw data from tables and from other views.
  DBSP provides powerful SQL analysis features, including time-series
  operators.

A program defines a computation, but it doesn't specify the source or
destination for data.  Those are the province of **connectors** and
**pipelines**, described below.

### Connectors

A **connector** gives a DBSP computation access to data.  There are
two classes of connectors: **input connectors** that connect a source
of input records to a DBSP table, and **output connectors** that
connect a DSBP view to a destination.

DBSP includes input and output connectors for [Kafka], open source
event streaming software from the [Apache Software
Foundation][Apache].  Kafka's API is widely adopted, which means that
these connectors also allow DBSP to work directly with [RedPanda] and
other software that use the same API.

DBSP has a plug-in connector model.  This means that connectors can be
written to interface with whatever data sources and sinks a user would
find most convenient.

[Kafka]: https://kafka.apache.org/
[Apache]: https://www.apache.org/
[RedPanda]: https://redpanda.com/

### Pipelines

A user assembles a **pipeline** by attaching a program's tables to
input connectors and its views to output connectors.  A DBSP pipeline
is the top-level unit of execution.  Once a DBSP user assembles a
pipeline from a program and connectors, they may start, stop, manage,
and monitor it.

## Foundations

DBSP is the pioneering implementation of a new theory that unifies
databases, streaming computation, and incremental view maintenance,
written by the inventors of that theory.  See our
[publications](papers) for all the details.

The DBSP code is available on [Github][DBSP] using an MIT open-source
license.  It consists of a Rust runtime and a SQL compiler.

[DBSP]: https://github.com/feldera/dbsp
