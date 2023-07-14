# Get Started

These instructions explain how to run DBSP on a single machine in a
configuration suitable for demos, development, and testing.  For
production use, or for developing DBSP itself, DBSP supports other
forms of deployment.

## Install prerequisites

To run the demo, you will first need to install Docker and the Docker
Compose plugin.  If you don't already have them, install them one of
these ways:

* On Mac OS, Windows, or Linux, install [Docker Desktop][1], which
  includes the Docker Compose plugin.

* On Linux only, first install [Docker Engine][2] and the [Docker
  Compose plugin][3].

  :::tip

  The plugin implements Docker Compose v2, invoked as `docker
  compose`.  DBSP does not support the older Docker Compose v1,
  which was invoked with `docker-compose` (note the ` ` versus `-`
  distinction).

  :::

  Then, follow the instructions to [manage Docker as a non-root
  user][4].

  :::tip

  If you you only want root to manage Docker, you can prefix
  the `docker compose` command below with `sudo`.

  :::

You also need `curl` and a web browser such as Chrome or Firefox.

[1]: https://docs.docker.com/desktop/
[2]: https://docs.docker.com/engine/install/
[3]: https://docs.docker.com/compose/install/linux
[4]: https://docs.docker.com/engine/install/linux-postinstall/

## Start the DBSP demo

1. Download the `docker-compose.yml` file that tells Docker Compose
   how to set up the demo's containers:

   ```bash
   curl -O https://raw.githubusercontent.com/feldera/dbsp/main/deploy/docker-compose.yml
   ```

   You only need to do the first time.

2. Launch containers for the demo, including the DBSP manager,
   Postgres to support it, and a SecOps demo that uses Redpanda:

   ```bash
   docker compose --profile demo up
   ```

   This command takes over the terminal where you run it as long as
   the DBSP demo is active.  It will print a lot of log messages,
   which can be useful for debugging if something goes wrong but which
   otherwise are not important.

   The first time you run this command, it will download container
   images, which can take a while.  Once `docker compose` begins
   bringing up the images, it takes about 10 seconds for the DBSP user
   interface to become available.  On fast systems, this includes a
   pause of a few seconds in which nothing is logged.

3. Visit <http://localhost:8085/> in your web browser to bring up the
   DBSP web-based user interface.

4. Try out one of the [demos](category/demos).

## Stop the DBSP demo

To shut down the DBSP demo, type <kbd>Ctrl+C</kbd> in the terminal
where `docker compose` is running.  Docker Compose will show its
progress as it shuts down each of the containers in turn.  It can take
up to about 15 seconds to complete shutdown.

:::caution

Stopping the demo discards SQL and anything else that you added, so be
sure that you copy out anything valuable before you stop it.

:::

## Troubleshooting

If the demo fails to start after it previously ran successfully, then
it might not have fully shut down from the previous run.  To ensure
that it is fully shut down, run the `docker compose` command from
before using `down` instead of `up`, and then try starting it again.

If starting the demo fails with `bind: address already in use`, then
something on your machine is already listening on one of the ports
required for the demo.  The message should say the particular port.
To correct the problem, find and stop the process listening on the
port, or edit a copy of `docker-compose.yml` to use a different port
number.
