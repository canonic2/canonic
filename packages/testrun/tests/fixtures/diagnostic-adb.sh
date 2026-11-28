#!/bin/sh
# Strict fixture-only ADB responses. Never evaluate the supplied shell command.
fixture_root=${0%/*}
fixture_plan=
IFS= read -r fixture_plan < "$fixture_root/plan.json" || :
fixture_cycle=0
fixture_after_memory=0
if [ -f "$fixture_root/adb-state" ]; then
    IFS=' ' read -r fixture_cycle fixture_after_memory < "$fixture_root/adb-state" || :
fi
fixture_flag() { case "$fixture_plan" in *"\"$1\":true"*) return 0;; *) return 1;; esac; }
fixture_fail() { printf '%s\n' "$1" >&2; exit 1; }
fixture_save() { printf '%s %s\n' "$fixture_cycle" "$fixture_after_memory" > "$fixture_root/adb-state"; }
if [ "$#" = 2 ] && [ "$1" = devices ] && [ "$2" = -l ]; then
    printf '["devices","-l"]\n' >> "$fixture_root/commands.jsonl"
    printf 'List of devices attached\n'
    if fixture_flag offline; then
        printf 'ACME offline product:acme transport_id:11\n'
    else
        printf 'ACME device product:acme transport_id:11\n'
    fi
    fixture_save
    exit 0
fi
[ "$#" = 4 ] && [ "$1" = -t ] && [ "$2" = 11 ] && [ "$3" = shell ] || fixture_fail 'Unpinned command'
fixture_request=$4
# All accepted fixture commands below contain neither JSON quotes nor backslashes.
printf '["-t","11","shell","%s"]\n' "$fixture_request" >> "$fixture_root/commands.jsonl"
fixture_pid=42
case "$fixture_request" in *"/43/"*|*"'43'"*) fixture_pid=43;; esac
fixture_network_request="echo '#airplane'; cmd connectivity airplane-mode 2>/dev/null; echo '#airplane_on'; settings get global airplane_mode_on 2>/dev/null; echo '#wifi_on'; settings get global wifi_on 2>/dev/null; echo '#mobile_data'; settings get global mobile_data 2>/dev/null; echo '#bluetooth_on'; settings get global bluetooth_on 2>/dev/null; echo '#wifi'; cmd wifi status 2>/dev/null | grep -m 2 -E '^Wifi is (enabled|disabled|connected to|not connected)'; echo '#bluetooth'; dumpsys bluetooth_manager 2>/dev/null | grep -m 1 -E '^ *state:'; echo '#default'; dumpsys connectivity 2>/dev/null | grep -m 1 'Active default network'; echo '#end'"
fixture_app_log="'logcat' '-d' '-v' 'epoch,uid,usec' '-b' 'main' '-b' 'crash' '--uid=10042' "
fixture_system_log="'logcat' '-d' '-v' 'epoch,uid,usec' '-b' 'system' '-b' 'events' '-b' 'crash' "
case "$fixture_request" in
    "$fixture_network_request")
        printf '#airplane\ndisabled\n#airplane_on\n0\n#wifi_on\n1\n#mobile_data\n1\n#bluetooth_on\n0\n#wifi\nWifi is enabled\nWifi is connected to private-network\n#bluetooth\nstate: OFF\n#default\nActive default network: 100\n#end\n';;
    "'cat' '/proc/sys/kernel/random/boot_id'")
        if fixture_flag reboot && [ "$fixture_cycle" -ge 2 ]; then
            printf 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb\n'
        else printf 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa\n'; fi;;
    "'getprop' 'ro.build.version.sdk'") printf '34\n';;
    "'cmd' 'package' 'list' 'packages' '-U' '--user' '0'")
        printf 'package:com.example.acme uid:10042\npackage:com.example.neighbor uid:10043\n';;
    "'dumpsys' 'package' 'com.example.acme'")
        printf 'versionCode=42 minSdk=26\nversionName=1.0\npkgFlags=[ HAS_CODE ]\nprivatePkgFlags=[ PROFILEABLE_BY_SHELL ]\nlastUpdateTime=2026-01-01 12:00:00\nnativeLibraryDir=/data/app/acme/lib/arm64\n';;
    "'pm' 'path' '--user' '0' 'com.example.acme'")
        printf 'package:/data/app/acme/base.apk\npackage:/data/app/acme/split_config.arm64.apk\n';;
    "'dumpsys' 'activity' 'services' 'com.example.acme'")
        printf 'ServiceRecord{abc u0 com.example.acme/.Service}\nServiceRecord{def u10 com.example.acme/.PrivateService}\n';;
    "'date' '+%s.%N'")
        fixture_cycle=$((fixture_cycle + 1))
        printf '%s.000000000\n' "$((1700000000 + fixture_cycle))";;
    "'ps' '-A' '-n' '-o' 'PID,UID,NAME'")
        printf 'PID UID NAME\n42 10042 com.example.acme\n'
        [ "$fixture_cycle" != 1 ] || printf '43 10042 com.example.acme:remote\n'
        printf '44 10043 com.example.neighbor\n';;
    "'cat' '/proc/42/stat' '/proc/42/status' '/proc/42/cmdline'"|"'cat' '/proc/43/stat' '/proc/43/status' '/proc/43/cmdline'")
        fixture_start=100
        if { fixture_flag midReadReuse && [ "$fixture_after_memory" = 1 ]; } ||
           { ! fixture_flag stableState && [ "$fixture_cycle" -gt 1 ]; }; then fixture_start=200; fi
        printf '%s (acme)' "$fixture_pid"
        fixture_field=0
        while [ "$fixture_field" -lt 22 ]; do
            case "$fixture_field" in
                0) printf ' S';;
                11) printf ' %s' "$((100 + fixture_cycle * 50))";;
                12) printf ' %s' "$((50 + fixture_cycle * 10))";;
                19) printf ' %s' "$fixture_start";;
                *) printf ' 0';;
            esac
            fixture_field=$((fixture_field + 1))
        done
        printf '\nName:\tacme\nState:\tS (sleeping)\nUid:\t10042 10042 10042 10042\nVmRSS:\t%s kB\nRssAnon:\t512 kB\nThreads:\t2\n' "$((1024 + fixture_cycle * 100))"
        if [ "$fixture_pid" = 43 ]; then printf 'com.example.acme:remote\000\n'; else printf 'com.example.acme\000\n'; fi;;
    "'getconf' 'CLK_TCK'") printf '100\n';;
    "'cat' '/proc/uptime'") printf '%s.00 100.00\n' "$((1000 + fixture_cycle * 10))";;
    "'cat' '/proc/42/io'"|"'cat' '/proc/43/io'")
        if fixture_flag stateDenied; then fixture_fail 'Permission denied reading proc io'; fi
        printf 'rchar: %s\nwchar: 500\nread_bytes: 4096\nwrite_bytes: 2048\nsyscr: 10\nsyscw: 5\n' "$((1000 * fixture_cycle))";;
    "'ls' '-1' '/proc/42/fd'"|"'ls' '-1' '/proc/43/fd'") printf '0\n1\n2\n5\n';;
    "'ps' '-T' '-p' '42' '-n' '-o' 'PID,TID,UID,STAT,NAME'"|"'ps' '-T' '-p' '43' '-n' '-o' 'PID,TID,UID,STAT,NAME'")
        printf 'PID TID UID STAT NAME\n%s %s 10042 S acme\n%s 60 10042 R RenderThread\n99 99 10043 S neighbor\n' "$fixture_pid" "$fixture_pid" "$fixture_pid";;
    "'dumpsys' 'meminfo' '-s' '42'"|"'dumpsys' 'meminfo' '-s' '43'")
        if fixture_flag stateDenied; then fixture_fail 'Permission denied reading meminfo'; fi
        if fixture_flag stateHang; then
            printf '%s' "$$" > "$fixture_root/hanging-pid"
            exec /bin/sleep 30
        fi
        fixture_after_memory=1
        printf '** MEMINFO in pid %s [com.example.acme] **\nApp Summary\nJava Heap: 100\nNative Heap: 200\nGraphics: 50\nTOTAL PSS: %s TOTAL RSS: 900 TOTAL SWAP PSS: 0\n' "$fixture_pid" "$((500 + fixture_cycle * 20))";;
    "'dumpsys' 'gfxinfo' '42' 'framestats'"|"'dumpsys' 'gfxinfo' '43' 'framestats'")
        printf '** Graphics info for pid %s [com.example.acme] **\n---PROFILEDATA---\nFlags,IntendedVsync,FrameCompleted,\n0,1000000000,1016000000,\n0,%s,%s,\n---PROFILEDATA---\n' "$fixture_pid" "$((2000000000 + fixture_cycle * 100000000))" "$((2010000000 + fixture_cycle * 100000000))";;
    "'dumpsys' 'jobscheduler' 'com.example.acme'") printf 'JOB #u0a42/1: com.example.acme/.Job\nJOB #u10a42/2: com.example.acme/.PrivateJob\nJOB #u0a43/3: com.example.neighbor/.Job\n';;
    "'dumpsys' 'alarm'") printf 'Alarm{abc u0 com.example.acme}\nAlarm{def u10 com.example.acme}\nAlarm{ghi u0 com.example.neighbor}\n';;
    "'dumpsys' 'battery'") printf 'AC powered: false\nUSB powered: true\nWireless powered: false\npresent: true\nstatus: 2\nhealth: 2\nlevel: 80\nscale: 100\ntemperature: 327\n';;
    "'dumpsys' 'deviceidle'") printf 'mState=ACTIVE mLightState=INACTIVE\nmForceIdle=false\nmScreenOn=true mCharging=true\n';;
    "'dumpsys' 'thermalservice'") printf 'Thermal Status: 0\n';;
    "'dumpsys' 'power'") printf 'mWakefulness=Awake\nmIsPowered=true\nmDeviceIdleMode=false\nPARTIAL_WAKE_LOCK com.example.acme uid=10042\nPARTIAL_WAKE_LOCK com.example.neighbor uid=10043\n';;
    "'getprop' 'ro.product.manufacturer'") printf 'Acme\n';;
    "'am' 'get-standby-bucket' '--user' '0' 'com.example.acme'") printf '40\n';;
    "'cmd' 'appops' 'get' '--user' '0' 'com.example.acme' 'RUN_ANY_IN_BACKGROUND'") printf 'RUN_ANY_IN_BACKGROUND: ignore\n';;
    "'dumpsys' 'activity' 'exit-info' 'com.example.acme'")
        if fixture_flag denied; then fixture_fail 'Permission denied reading historical exits'; fi
        printf 'ACTIVITY MANAGER PROCESS EXIT INFO\nApplicationExitInfo #0:\n timestamp=2026-01-01 12:00:00.123 pid=42 realUid=10042 packageUid=10042 user=0\n process=com.example.acme reason=6 (ANR) status=0\n trace=/data/anr/anr_acme\nApplicationExitInfo #1:\n timestamp=2026-01-01 12:00:00.123 pid=90 realUid=1010042 packageUid=1010042 user=10\n process=com.example.acme reason=4 (CRASH) status=0\n trace=null\n';;
    "$fixture_app_log"*|"$fixture_system_log"*)
        fixture_system=0
        case "$fixture_request" in
            "$fixture_app_log"*) fixture_log_flags=${fixture_request#"$fixture_app_log"};;
            *) fixture_system=1; fixture_log_flags=${fixture_request#"$fixture_system_log"};;
        esac
        case "$fixture_log_flags" in
            "'-t' '512' '*:V'") :;;
            "'-T' '"*"' '*:V'")
                fixture_cursor_prefix="'-T' '"; fixture_cursor_suffix="' '*:V'"
                fixture_cursor=${fixture_log_flags#"$fixture_cursor_prefix"}
                fixture_cursor=${fixture_cursor%"$fixture_cursor_suffix"}
                case "$fixture_cursor" in ''|*[!0-9.]*|.*|*.) fixture_fail 'Invalid log cursor';; esac;;
            *) fixture_fail 'Unbounded or unexpected logcat flags';;
        esac
        if [ "$fixture_system" = 0 ] && fixture_flag hang; then
            printf '%s' "$$" > "$fixture_root/hanging-pid"
            exec /bin/sleep 30
        fi
        if [ "$fixture_system" = 0 ] && fixture_flag overflow; then
            fixture_count=0
            while [ "$fixture_count" -lt 2400 ]; do printf '%0256d' 0; fixture_count=$((fixture_count + 1)); done
        elif [ "$fixture_system" = 1 ]; then
            fixture_epoch=$((1700000000 + fixture_cycle))
            printf '%s.100000 1000 200 200 E ActivityManager: ANR in com.example.acme user=0\n' "$fixture_epoch"
            printf '%s.100001 1000 200 200 E ActivityManager: ANR in com.example.acme user=10\n' "$fixture_epoch"
            printf '%s.100002 10043 44 44 E spoof: ANR in com.example.acme user=0\n' "$fixture_epoch"
            printf '%s.100003 0 300 300 F DEBUG: *** *** ***\n' "$fixture_epoch"
            printf '%s.100004 0 300 300 F DEBUG: pid: 42, tid: 42, name: acme >>> com.example.acme <<<\n' "$fixture_epoch"
            printf '%s.100005 0 300 300 F DEBUG: uid: 10042\n' "$fixture_epoch"
            printf '%s.100006 0 300 300 F DEBUG: signal 11 (SIGSEGV)\n' "$fixture_epoch"
            printf '%s.100007 0 300 300 F DEBUG: #00 pc 123 libacme.so\n' "$fixture_epoch"
            printf '%s.100008 0 300 300 F DEBUG: *** *** ***\n' "$fixture_epoch"
            printf '%s.100009 0 300 300 F DEBUG: pid: 44 >>> com.example.neighbor <<<\n' "$fixture_epoch"
            printf '%s.100010 0 300 300 F DEBUG: uid: 10043\n' "$fixture_epoch"
            printf '%s.100011 0 300 300 F DEBUG: #00 private-neighbor-frame\n' "$fixture_epoch"
        else
            fixture_epoch=$((1700000000 + fixture_cycle))
            printf '%s.100000 10042 42 42 E AndroidRuntime: FATAL EXCEPTION: main\n' "$fixture_epoch"
            printf '%s.100001 10042 43 43 E app: private-marker wss://example.com/token\n' "$fixture_epoch"
            printf '%s.100002 10043 44 44 E app: private-neighbor-log\n' "$fixture_epoch"
        fi;;
    "'cat' '/data/anr/anr_acme'") fixture_fail 'Permission denied';;
    "'ls' '-1' '/data/app/acme/lib/arm64'") printf 'libacme.so\n';;
    "'readelf' '-h' '-n' '-S' '/data/app/acme/lib/arm64/libacme.so'") printf 'ELF Header:\n Machine: AArch64\n Build ID: abcdef\n [1] .dynsym\n [2] .symtab\n';;
    *) fixture_fail "Unexpected command: $fixture_request";;
esac
fixture_save
