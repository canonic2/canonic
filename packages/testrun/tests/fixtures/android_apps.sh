#!/bin/sh
# Runs the shipping inventory shell script against a private fake remote root.
root=$(dirname "$0")
phase=$(/bin/cat "$root/phase")
if [ "$(basename "$0")" = cat ]; then
    touch "$root/push"
    if [ "$phase" = push ]; then
        while [ ! -e "$root/release" ]; do sleep 0.01; done
    fi
    /bin/cat
    case "$phase" in push_failed|cleanup_failed) exit 7 ;; esac
    exit 0
fi
if [ "$(basename "$0")" = app_process ]; then
    touch "$root/helper"
    if [ "$phase" = helper ]; then
        while [ ! -e "$root/release" ]; do sleep 0.01; done
    fi
    [ "$phase" != helper_failed ] || exit 7
    printf '%s\n' '{"version":1,"ok":true,"data":{"apps":[{"appId":"com.example.acme","name":"Acme","launchable":true}]}}'
    exit 0
fi
case "$3" in
    shell)
        if [ "$4" = -T ]; then shift; fi
        case "$4" in
            "'rm' '-f'"*)
                [ "$phase" != cleanup_failed ] || exit 7
                script=$(printf '%s' "$4" | sed "s|/data/local/tmp|$root|g")
                /bin/sh -c "$script" || exit 7
                touch "$root/cleaned"
                ;;
            "'cmd' 'package'"*) printf '%s\n' 'package:com.example.acme versionCode:1' ;;
            *)
                script=$(printf '%s' "$4" | sed "s|/data/local/tmp|$root|g")
                remote=$(printf '%s' "$script" | sed "s/.*remote='\([^']*\)'.*/\1/")
                printf '%s\n' "$remote" > "$root/remote"
                PATH="$root:$PATH" /bin/sh -c "$script"
                status=$?
                exit "$status"
                ;;
        esac
        ;;
    *) exit 1 ;;
esac
