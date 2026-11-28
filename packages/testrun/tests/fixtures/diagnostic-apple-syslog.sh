#!/bin/sh
set -eu

root=${0%/*}
if [ "$#" -eq 1 ] && [ "$1" = --version ]; then
    printf '%s' 'xcrun version fixture-1'
    exit 0
fi

fail() {
    printf '%s\n' "$1" >&2
    exit 1
}

IFS=' ' read -r network no_support service_denied < "$root/apple-syslog-plan"
filter='SpringBoard|runningboardd|backboardd|mediaserverd|audiomxd|powerd|thermalmonitord|ReportCrash'
if [ "$no_support" = false ]; then
    filter="42|$filter"
fi
if [ "$network" = true ]; then
    [ "$#" -eq 7 ] && [ "$7" = --network ] || fail 'Unexpected device log flags'
else
    [ "$#" -eq 6 ] || fail 'Unexpected device log flags'
fi
[ "$1" = --udid ] && [ "$2" = ACME ] && [ "$3" = --exit ] && [ "$4" = --no-colors ] &&
    [ "$5" = --process ] && [ "$6" = "$filter" ] || fail 'Unexpected device log flags'
[ "$service_denied" = false ] || fail 'Device is locked or service permission denied'

printf '%s\n' \
    'Nov 7 10:30:01 Phone Acme[42] <Notice>: app log private-token' \
    'Nov 7 10:30:02 Phone runningboardd[8] <Notice>: watchdog com.example.acme' \
    'Nov 7 10:30:03 Phone Other[99] <Notice>: private-neighbor'
# Keep the producer open under the same PID until Testrun settles the sample.
printf '%s' "$$" > "$root/apple-hanging-pid"
exec /bin/sleep 60
