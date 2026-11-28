/* Unicode 16's identical 22-byte White_Space table predates the index field.
 * Its final range ends at U+3000, so the sole index sentinel is U+3001.
 * Keep the pinned data header byte-identical to upstream. */
static const uint8_t unicode_prop_White_Space_index[3] = {0x01, 0x30, 0x00};
