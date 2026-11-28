/* Owned allocation, stack and cancellation boundary for the standalone engine. */
#include "bridge.h"
#include "vendor/libregexp.h"
#include <limits.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>

#define TR_MEMORY (32u * 1024u * 1024u)
#define TR_STACK (512u * 1024u)
typedef union tr_allocation tr_allocation;
union tr_allocation {
    struct { size_t size; tr_allocation *previous, *next; } entry;
#if defined(_MSC_VER)
    /* Keep the payload aligned to the Windows CRT allocation boundary. */
#if defined(_WIN64)
    __declspec(align(16)) unsigned char alignment;
#else
    __declspec(align(8)) unsigned char alignment;
#endif
#else
    max_align_t alignment;
#endif
};
typedef struct {
    size_t used;
    tr_allocation *head;
    uintptr_t stack;
    int bounded, interrupted;
    int (*stopped)(void *);
    void *control;
} tr_cookie_runtime;

int lre_check_timeout(void *opaque) {
    tr_cookie_runtime *runtime = opaque;
    if (runtime->stopped(runtime->control)) runtime->interrupted = 1;
    return runtime->interrupted;
}
bool lre_check_stack_overflow(void *opaque, size_t extra) {
    tr_cookie_runtime *runtime = opaque;
    const uintptr_t here = (uintptr_t)&runtime;
    size_t used = here > runtime->stack ? here - runtime->stack : runtime->stack - here;
    if (extra > TR_STACK || used > TR_STACK - extra) {
        runtime->bounded = 1;
        return true;
    }
    return false;
}
void *lre_realloc(void *opaque, void *pointer, size_t size) {
    tr_cookie_runtime *runtime = opaque;
    tr_allocation *old = pointer ? (tr_allocation *)pointer - 1 : NULL;
    const size_t old_total = old ? old->entry.size + sizeof(*old) : 0;
    if (!size) {
        if (old) {
            if (old->entry.previous) old->entry.previous->entry.next = old->entry.next;
            else runtime->head = old->entry.next;
            if (old->entry.next) old->entry.next->entry.previous = old->entry.previous;
            runtime->used -= old_total;
            free(old);
        }
        return NULL;
    }
    if (lre_check_timeout(runtime)) return NULL;
    if (size > TR_MEMORY - sizeof(tr_allocation) ||
        size + sizeof(tr_allocation) > TR_MEMORY - (runtime->used - old_total)) {
        runtime->bounded = 1;
        return NULL;
    }
    tr_allocation *previous = old ? old->entry.previous : NULL;
    tr_allocation *next = old ? old->entry.next : runtime->head;
    tr_allocation *fresh = realloc(old, sizeof(*fresh) + size);
    if (!fresh) { runtime->bounded = 1; return NULL; }
    fresh->entry.size = size;
    fresh->entry.previous = previous;
    fresh->entry.next = next;
    if (previous) previous->entry.next = fresh;
    else runtime->head = fresh;
    if (next) next->entry.previous = fresh;
    runtime->used = runtime->used - old_total + sizeof(*fresh) + size;
    return fresh + 1;
}
static int parse_flag_bytes(const uint8_t *bytes, size_t length) {
    int flags = 0;
    for (size_t index = 0; index < length; index++) {
        int flag;
        switch (bytes[index]) {
            case 'd': flag = LRE_FLAG_INDICES; break;
            case 'g': flag = LRE_FLAG_GLOBAL; break;
            case 'i': flag = LRE_FLAG_IGNORECASE; break;
            case 'm': flag = LRE_FLAG_MULTILINE; break;
            case 's': flag = LRE_FLAG_DOTALL; break;
            case 'u': flag = LRE_FLAG_UNICODE; break;
            case 'v': flag = LRE_FLAG_UNICODE_SETS; break;
            case 'y': flag = LRE_FLAG_STICKY; break;
            default: return -1;
        }
        if (flags & flag) return -1;
        flags |= flag;
    }
    if ((flags & LRE_FLAG_UNICODE) && (flags & LRE_FLAG_UNICODE_SETS)) return -1;
    return flags;
}
static int parse_flags(const tr_cookie_criterion *criterion) {
    return parse_flag_bytes(criterion->flags, criterion->flags_length);
}
static int compile_status(const tr_cookie_runtime *runtime, const char *error) {
    return runtime->interrupted ? 3 : runtime->bounded ||
        strstr(error, "too many captures") || strstr(error, "too many ranges") ||
        strstr(error, "too many registers") || strstr(error, "too large") ? 2 : 1;
}
static int exec_status(const tr_cookie_runtime *runtime, int result) {
    return runtime->interrupted || result == LRE_RET_TIMEOUT ? 3 :
        runtime->bounded || result == LRE_RET_MEMORY_ERROR ? 2 : 4;
}
static int filter(const tr_cookie_criterion criteria[3],
                     const tr_cookie_attributes *cookies, size_t count,
                     uint8_t *matches, int (*stopped)(void *), void *control,
                     bool codepoint_reference) {
    tr_cookie_runtime runtime = {0};
    runtime.stack = (uintptr_t)&runtime;
    runtime.stopped = stopped;
    runtime.control = control;
    uint8_t *programs[3] = {0};
    uint8_t **captures[3] = {0};
    int status = 0;
    if ((!codepoint_reference && count > 4096) || !stopped) return 2;
    for (size_t field = 0; field < 3; field++) {
        const tr_cookie_criterion *criterion = &criteria[field];
        if (lre_check_timeout(&runtime)) { status = 3; goto cleanup; }
        if (criterion->kind != 2) continue;
        int flags = parse_flags(criterion);
        if (flags < 0) { status = 1; goto cleanup; }
        int length;
        char error[128] = {0};
        programs[field] = lre_compile(&length, error, sizeof(error),
            (const char *)criterion->pattern, criterion->pattern_length, flags, &runtime);
        if (!programs[field]) {
            status = compile_status(&runtime, error);
            goto cleanup;
        }
        if (lre_check_bytecode(programs[field], length)) { status = 4; goto cleanup; }
        int groups = lre_get_capture_count(programs[field]);
        int slots = lre_get_alloc_count(programs[field]);
        if (groups <= 0 || slots < groups * 2 || slots > 65536) { status = 4; goto cleanup; }
        captures[field] = lre_realloc(&runtime, NULL, (size_t)slots * sizeof(uint8_t *));
        if (!captures[field]) { status = runtime.interrupted ? 3 : 2; goto cleanup; }
        memset(captures[field], 0, (size_t)slots * sizeof(uint8_t *));
    }
    for (size_t index = 0; index < count; index++) {
        int matched = 1;
        uint16_t units[3] = {0};
        tr_cookie_attributes generated = {0};
        if (codepoint_reference) {
            size_t length = 1;
            if (index <= 0xffff) units[0] = (uint16_t)index;
            else {
                const uint32_t code = (uint32_t)index - 0x10000;
                units[0] = (uint16_t)(0xd800 + (code >> 10));
                units[1] = (uint16_t)(0xdc00 + (code & 1023));
                length = 2;
            }
            generated.fields[0].data = units;
            generated.fields[0].length = length;
        }
        for (size_t field = 0; field < 3; field++) {
            if (lre_check_timeout(&runtime)) { status = 3; goto cleanup; }
            const tr_cookie_criterion *criterion = &criteria[field];
            const tr_cookie_string *value = codepoint_reference ?
                &generated.fields[field] : &cookies[index].fields[field];
            if (value->length > INT_MAX / 2) { status = 2; goto cleanup; }
            if (criterion->kind == 0) continue;
            if (criterion->kind == 1) {
                matched = value->length == criterion->exact.length &&
                    (!value->length || !memcmp(value->data, criterion->exact.data, value->length * sizeof(uint16_t)));
            } else {
                int result = lre_exec(captures[field], programs[field],
                    (const uint8_t *)value->data, 0, (int)value->length, 1, &runtime);
                if (result < 0) {
                    status = exec_status(&runtime, result);
                    goto cleanup;
                }
                matched = result != 0;
            }
            if (!matched) break;
        }
        matches[index] = (uint8_t)matched;
    }
cleanup:
    /* Also reclaim allocations left by an upstream compile/match error. */
    while (runtime.head) lre_realloc(&runtime, runtime.head + 1, 0);
    return status;
}

int tr_cookie_filter(const tr_cookie_criterion criteria[3],
                     const tr_cookie_attributes *cookies, size_t count,
                     uint8_t *matches, int (*stopped)(void *), void *control) {
    return filter(criteria, cookies, count, matches, stopped, control, false);
}

/* The program's allocations stay in its compile runtime until tr_regexp_free;
 * each test uses its own runtime for captures and backtracking. */
struct tr_regexp {
    tr_cookie_runtime runtime;
    uint8_t *program;
    int slots;
};

void tr_regexp_free(tr_regexp *regexp) {
    if (!regexp) return;
    while (regexp->runtime.head) lre_realloc(&regexp->runtime, regexp->runtime.head + 1, 0);
    free(regexp);
}

int tr_regexp_compile(const uint8_t *pattern, size_t pattern_length,
                      const uint8_t *flag_bytes, size_t flags_length,
                      int (*stopped)(void *), void *control, tr_regexp **result) {
    if (!result || !stopped || pattern_length > INT_MAX) return 2;
    *result = NULL;
    const int flags = parse_flag_bytes(flag_bytes, flags_length);
    if (flags < 0) return 1;
    tr_regexp *regexp = calloc(1, sizeof(*regexp));
    if (!regexp) return 2;
    regexp->runtime.stack = (uintptr_t)&regexp;
    regexp->runtime.stopped = stopped;
    regexp->runtime.control = control;
    int length, status = 0;
    char error[128] = {0};
    regexp->program = lre_compile(&length, error, sizeof(error),
        (const char *)pattern, pattern_length, flags, &regexp->runtime);
    if (!regexp->program) status = compile_status(&regexp->runtime, error);
    else if (lre_check_bytecode(regexp->program, length)) status = 4;
    else {
        const int groups = lre_get_capture_count(regexp->program);
        regexp->slots = lre_get_alloc_count(regexp->program);
        if (groups <= 0 || regexp->slots < groups * 2 || regexp->slots > 65536) status = 4;
    }
    if (status) {
        tr_regexp_free(regexp);
        return status;
    }
    /* The compile runtime only frees from here; no callback outlives this call. */
    regexp->runtime.stopped = NULL;
    regexp->runtime.control = NULL;
    *result = regexp;
    return 0;
}

int tr_regexp_test(const tr_regexp *regexp, const uint16_t *subject, size_t length,
                   int (*stopped)(void *), void *control) {
    if (!regexp || !stopped || length > INT_MAX / 2) return 2;
    tr_cookie_runtime runtime = {0};
    runtime.stack = (uintptr_t)&runtime;
    runtime.stopped = stopped;
    runtime.control = control;
    int status;
    uint8_t **captures = lre_realloc(&runtime, NULL, (size_t)regexp->slots * sizeof(uint8_t *));
    if (!captures) {
        status = runtime.interrupted ? 3 : 2;
    } else {
        memset(captures, 0, (size_t)regexp->slots * sizeof(uint8_t *));
        const int result = lre_exec(captures, regexp->program, (const uint8_t *)subject,
            0, (int)length, 1, &runtime);
        status = result < 0 ? exec_status(&runtime, result) : result != 0;
    }
    while (runtime.head) lre_realloc(&runtime, runtime.head + 1, 0);
    return status;
}

/* Reference harness only: compile once, classify every Unicode code point.
 * There is no tool dispatch to this entry point. Production retains 4096 cookies. */
int tr_cookie_filter_codepoints(const tr_cookie_criterion criteria[3],
                     uint8_t *matches, int (*stopped)(void *), void *control) {
    return filter(criteria, NULL, 0x110000, matches, stopped, control, true);
}
