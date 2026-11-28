#ifndef TESTRUN_COOKIE_REGEXP_H
#define TESTRUN_COOKIE_REGEXP_H
#include <stddef.h>
#include <stdint.h>
typedef struct { const uint16_t *data; size_t length; } tr_cookie_string;
typedef struct {
    int kind; /* 0 omitted, 1 exact, 2 RegExp */
    tr_cookie_string exact;
    const uint8_t *pattern;
    size_t pattern_length;
    const uint8_t *flags;
    size_t flags_length;
} tr_cookie_criterion;
typedef struct { tr_cookie_string fields[3]; } tr_cookie_attributes;
/* 0 success, 1 syntax, 2 resource bound, 3 interrupted, 4 engine failure. */
int tr_cookie_filter(const tr_cookie_criterion criteria[3],
                     const tr_cookie_attributes *cookies, size_t count,
                     uint8_t *matches, int (*stopped)(void *), void *control);
/* A compiled RegExp program, immutable after compilation and shareable for tests. */
typedef struct tr_regexp tr_regexp;
/* 0 success, 1 syntax, 2 resource bound, 3 interrupted, 4 engine failure. */
int tr_regexp_compile(const uint8_t *pattern, size_t pattern_length,
                      const uint8_t *flags, size_t flags_length,
                      int (*stopped)(void *), void *control, tr_regexp **result);
/* 0 no match, 1 match; otherwise 2 resource bound, 3 interrupted, 4 engine failure. */
int tr_regexp_test(const tr_regexp *regexp, const uint16_t *subject, size_t length,
                   int (*stopped)(void *), void *control);
void tr_regexp_free(tr_regexp *regexp);
#endif
