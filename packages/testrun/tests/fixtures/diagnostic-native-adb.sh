#!/bin/sh
# Independent native collector peer; no command evaluation or real device I/O.
printf '%s\n' "$*" >> "$0.calls"
if [ "$1" = devices ] && [ "$2" = -l ]; then
  printf 'List of devices attached\nACME device transport_id:7\n'
  exit 0
fi
[ "$1" = -t ] && [ "$2" = 7 ] || exit 20
case "$3:$4" in
  shell:-T)
    case "$5" in
      'perfetto --txt -c - -o -') cat > "$0.config" ;;
      'exec perfetto --txt -c - -o - --notify-fd 3 3>&2')
        cat > "$0.config"
        if [ -f "$0.refuse-start" ]; then exit 21; fi
        printf 'starting probe\n\000' >&2
        while [ ! -f "$0.release" ]; do sleep 0.02; done
        ;;
      *) exit 21 ;;
    esac
    printf 'owned perfetto bytes'
    : > "$0.recorded"
    ;;
  shell:*)
    case "$4" in
      "'cat' '/proc/sys/kernel/random/boot_id'"|"cat /proc/sys/kernel/random/boot_id") printf 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee\n' ;;
      "'cmd' 'package' 'list' 'packages' '--user' '0' '-U' 'com.example.acme'") printf 'package:com.example.acme uid:10042\n' ;;
      "'cat' '/proc/42/stat'")
        if [ -f "$0.exit-after" ] && [ -f "$0.recorded" ]; then exit 30; fi
        printf '42 (Acme) S 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 100 0\n'
        ;;
      "'cat' '/proc/42/status'") printf 'Name:\tAcme\nUid:\t10042 10042 10042 10042\n' ;;
      "'simpleperf' 'record' '-p' '42' '-e' 'cpu-clock:u' '-f' '99' '--call-graph' 'dwarf,8192' '--duration' '0.100' '-o' '/data/local/tmp/testrun-"*".perf.data'") printf 'owned simpleperf bytes' > "$0.perf" ;;
      "rm -- '/data/local/tmp/testrun-"*".perf.data'") : ;;
      *) exit 22 ;;
    esac
    ;;
  exec-out:cat)
    case "$5" in /data/local/tmp/testrun-*.perf.data) cat "$0.perf" ;; *) exit 23 ;; esac
    ;;
  *) exit 24 ;;
esac
