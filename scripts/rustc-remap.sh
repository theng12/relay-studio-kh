#!/bin/sh
# Adds the home-path remap at rustc time, so no committed file names a home directory. The prefix
# must be absolute, so $HOME is resolved here, on whichever machine builds. Compile invocations only:
# cargo's version probes get no extra flag.
case " $* " in
  *" --crate-name "*) exec "$@" "--remap-path-prefix=$HOME=/build" ;;
  *) exec "$@" ;;
esac
