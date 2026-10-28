/* SPDX-License-Identifier: GPL-3.0-or-later */

/*
 * pigeon core — C ABI
 *
 * GENERATED FILE — DO NOT EDIT.
 * Source of truth: core/src/ffi/ (mod.rs for the ABI, msg.rs for the message
 * codec). Regenerate from the repository root with
 * exactly the command CI runs:
 *
 *     cbindgen --config core/cbindgen.toml --crate pigeon-core \
 *              --output include/pigeon_core.h core
 *
 * CI (.github/workflows/ci.yml, job `header`) regenerates into a temp file and
 * `diff -u`s it against the committed one; a hand-edited header fails the build.
 *
 * Contract notes common to every function below:
 *   - All calls are synchronous and non-blocking; the core never performs IO.
 *   - All calls on one PgCore must be serialized by the caller (one thread at a
 *     time). A PgSnapshot is immutable and may be sent to another thread.
 *   - Strings are UTF-8 with an explicit length, never NUL-terminated.
 *   - Failures return NULL/0/false and set the thread-local error, read with
 *     pg_last_error(). A successful call clears it back to PG_OK, so a stale
 *     error can never be observed. A Rust panic is caught at the FFI boundary
 *     and reported as PG_ERR_PANIC; it never unwinds into C.
 *   - Every struct starts with `size` (sizeof as filled by the caller, or as
 *     written by the core) so fields can be appended without breaking ABI.
 *     Callers may pass a larger size than this header declares; the core
 *     zero-fills the unknown trailing bytes. A smaller size is rejected with
 *     PG_ERR_INVALID_ARG. New per-cell data may also go into `attrs` bits.
 */

typedef struct PgCore PgCore;
typedef struct PgMsg PgMsg;


#ifndef PIGEON_CORE_H
#define PIGEON_CORE_H



#include <stdarg.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>

/**
 * ABI generation. Bumped only for non-additive changes.
 */
#define PG_CORE_ABI_VERSION 1

/**
 * Attribute bits carried by [`PgCell::attrs`].
 */
#define PG_ATTR_BOLD (1 << 0)

#define PG_ATTR_ITALIC (1 << 1)

#define PG_ATTR_UNDERLINE (1 << 2)

#define PG_ATTR_REVERSE (1 << 3)

#define PG_ATTR_STRIKETHROUGH (1 << 4)

#define PG_ATTR_DIM (1 << 5)

/**
 * Default minimum size of a pane, in cells.
 */
#define MIN_COLS 10

#define MIN_ROWS 3

/**
 * A frame may not exceed 8 MiB (protocol §1).
 */
#define MAX_FRAME ((8 * 1024) * 1024)

/**
 * Exact wire version: major 1, minor 0.
 */
#define PROTO_VERSION 256

/**
 * At most this many records per pane.
 */
#define MAX_RECORDS 256

/**
 * Attribute bits, mirrored by `PG_ATTR_*` in the C ABI and by the protocol's
 * `cell[1]`.
 */
#define ATTR_BOLD (1 << 0)

#define ATTR_ITALIC (1 << 1)

#define ATTR_UNDERLINE (1 << 2)

#define ATTR_REVERSE (1 << 3)

#define ATTR_STRIKETHROUGH (1 << 4)

#define ATTR_DIM (1 << 5)

typedef enum PgAxis {
    /**
     * Panes side by side, vertical divider ("v" in the protocol).
     */
    PG_AXIS_VERTICAL = 0,
    /**
     * Panes stacked, horizontal divider ("h" in the protocol).
     */
    PG_AXIS_HORIZONTAL = 1,
} PgAxis;

/**
 * Result of the last call on this thread.
 */
typedef enum PgErrorCode {
    PG_OK = 0,
    /**
     * Caller passed a bad size/version/enum value.
     */
    PG_ERR_INVALID_ARG = 1,
    /**
     * `pane_id` does not exist.
     */
    PG_ERR_NO_SUCH_PANE = 2,
    /**
     * `split_id` does not exist.
     */
    PG_ERR_NO_SUCH_SPLIT = 3,
    /**
     * Allocation failed.
     */
    PG_ERR_NO_MEMORY = 4,
    /**
     * Output buffer too small; retry with the reported size.
     */
    PG_ERR_OVERFLOW = 5,
    /**
     * A panic was caught at the FFI boundary.
     */
    PG_ERR_PANIC = 6,
    /**
     * Valid request, not implemented in this build.
     */
    PG_ERR_UNSUPPORTED = 7,
} PgErrorCode;

/**
 * The M0 subset of `docs/protocol.md` §3.
 */
typedef enum PgMsgType {
    PG_MSG_UNKNOWN = 0,
    PG_MSG_HELLO = 1,
    PG_MSG_HELLO_ACK = 2,
    PG_MSG_HELLO_ERROR = 3,
    PG_MSG_SESSIONS = 4,
    PG_MSG_NEW_SESSION = 5,
    PG_MSG_CREATED = 6,
    PG_MSG_CLOSE_PANE = 7,
    PG_MSG_ATTACH = 8,
    PG_MSG_SNAPSHOT = 9,
    PG_MSG_DELTA = 10,
    PG_MSG_KEYS = 11,
    PG_MSG_RESIZE_PANE = 12,
    PG_MSG_PANE_EXITED = 13,
    PG_MSG_PING = 14,
    PG_MSG_PONG = 15,
    PG_MSG_ERROR = 16,
    PG_MSG_UNSUPPORTED = 17,
    PG_MSG_SPLIT_PANE = 18,
    PG_MSG_RESIZE_SPLIT = 19,
    PG_MSG_FOCUS = 20,
    PG_MSG_LAYOUT = 21,
    PG_MSG_STATUS = 22,
    PG_MSG_ACK_STATUS = 23,
    PG_MSG_APPEARANCE = 24,
    PG_MSG_SIZE_STATE = 25,
} PgMsgType;

typedef enum PgStatusKind {
    PG_KIND_NONE = 0,
    PG_KIND_PERMISSION = 1,
    PG_KIND_QUESTION = 2,
    PG_KIND_AUTH = 3,
} PgStatusKind;

typedef enum PgStatusState {
    PG_STATE_IDLE = 0,
    PG_STATE_WORKING = 1,
    PG_STATE_BLOCKED = 2,
    PG_STATE_DONE = 3,
    PG_STATE_ERROR = 4,
} PgStatusState;

/**
 * `[codepoint, attrs, fg, bg, link?]`. The fifth element is omitted when the
 * cell has no hyperlink (protocol §3.3); `null` is never emitted.
 */
typedef struct Cell Cell;

/**
 * UTF-8 bytes with an explicit length, never NUL-terminated.
 */
typedef struct PgStr {
    /**
     * `sizeof(PgStr)`.
     */
    uint32_t size;
    /**
     * Valid until the next core call on this thread.
     */
    const uint8_t *ptr;
    size_t len;
} PgStr;

typedef struct PgConfig {
    uint32_t size;
    uint32_t version;
    /**
     * 0 = core default (10000 lines); capped at 100000.
     */
    uint32_t scrollback_lines;
    uint8_t osc7501_enabled;
} PgConfig;

typedef struct PgPaneOptions {
    uint32_t size;
    uint32_t version;
    uint16_t cols;
    uint16_t rows;
    /**
     * Optional: `ptr` may be NULL.
     */
    struct PgStr title;
} PgPaneOptions;

typedef struct PgCursor {
    uint32_t size;
    uint16_t x;
    uint16_t y;
    uint8_t visible;
    /**
     * 0 block, 1 underline, 2 bar.
     */
    uint8_t shape;
} PgCursor;

typedef struct PgScrollRegion {
    uint32_t size;
    uint16_t top;
    uint16_t bottom;
} PgScrollRegion;

typedef struct PgCell {
    uint32_t size;
    uint32_t codepoint;
    /**
     * `0x01000000 | idx` for a palette entry, else `0xRRGGBB`.
     */
    uint32_t fg;
    uint32_t bg;
    /**
     * `PG_ATTR_*` bitmask.
     */
    uint32_t attrs;
    /**
     * 0 = no link.
     */
    uint32_t hyperlink_id;
    /**
     * 1 or 2; a wide char owns the next cell.
     */
    uint8_t width;
} PgCell;

typedef struct PgRow {
    uint32_t size;
    /**
     * Exactly `cols` entries.
     */
    const struct PgCell *cells;
} PgRow;

typedef struct PgSnapshot {
    uint32_t size;
    uint32_t version;
    uint32_t pane_id;
    uint16_t cols;
    uint16_t rows;
    uint8_t alt_screen;
    struct PgCursor cursor;
    struct PgScrollRegion scroll_region;
    /**
     * `rows` entries: the visible grid.
     */
    const struct PgRow *screen;
    /**
     * `history_count` entries, oldest first; may be NULL.
     */
    const struct PgRow *history;
    uint32_t history_count;
} PgSnapshot;

typedef struct PgStatus {
    uint32_t size;
    uint32_t version;
    uint32_t pane_id;
    /**
     * Monotonic per pane.
     */
    uint32_t seq;
    enum PgStatusState state;
    enum PgStatusKind kind;
    /**
     * Plain text only; never interpreted as markup.
     */
    struct PgStr msg;
} PgStatus;

typedef struct PgRect {
    uint32_t size;
    uint32_t pane_id;
    /**
     * Normalized 0..1, relative to the session box.
     */
    float x;
    float y;
    float w;
    float h;
} PgRect;

/**
 * A divider in a layout message: the box it divides plus its ratio.
 */
typedef struct PgSplit {
    uint32_t size;
    uint32_t id;
    enum PgAxis axis;
    float ratio;
    float x;
    float y;
    float w;
    float h;
} PgSplit;

/**
 * One entry of a `sessions` message. Strings are borrowed from the message.
 */
typedef struct PgSessionInfo {
    uint32_t size;
    uint32_t session_id;
    struct PgStr title;
    struct PgStr cwd;
    uint32_t pane_count;
    /**
     * 0 when the session has no focused pane.
     */
    uint32_t active_pane;
    struct PgStr active_title;
} PgSessionInfo;



#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

/**
 * Thread-local; empty and `PG_OK` before the first failure.
 */
struct PgStr pg_last_error(void);

/**
 * Code of the last call on this thread; `PG_OK` after a successful one.
 */
enum PgErrorCode pg_last_error_code(void);

/**
 * The ABI generation this library was built with.
 */
uint32_t pg_core_abi_version(void);

/**
 * Returns NULL on failure. `cfg == NULL` means defaults.
 */
PgCore *pg_core_new(const struct PgConfig *cfg);

/**
 * Frees a core created by [`pg_core_new`]; NULL is a no-op.
 */
void pg_core_free(PgCore *core);

/**
 * Creates a pane. Returns 0 on failure (ids start at 1). `opts == NULL` means
 * 80x24 with the core's scrollback setting.
 */
uint32_t pg_pane_new(PgCore *core, const struct PgPaneOptions *opts);

void pg_pane_free(PgCore *core, uint32_t pane_id);

/**
 * Parses bytes; never blocks, never echoes.
 */
bool pg_pane_feed(PgCore *core, uint32_t pane_id, const uint8_t *data, size_t len);

bool pg_pane_resize(PgCore *core, uint32_t pane_id, uint16_t cols, uint16_t rows);

/**
 * Pulls pending terminal replies (DA/DSR/…); returns bytes written.
 */
size_t pg_pane_respond(PgCore *core, uint32_t pane_id, uint8_t *out, size_t cap);

/**
 * Owned by the caller; free with [`pg_snapshot_free`]. Immutable, thread-safe.
 */
struct PgSnapshot *pg_pane_snapshot(PgCore *core, uint32_t pane_id);

/**
 * Fills row indices dirty since the last call; returns how many were written.
 */
size_t pg_pane_dirty_rows(PgCore *core, uint32_t pane_id, uint16_t *out, size_t cap);

void pg_snapshot_free(struct PgSnapshot *snap);

size_t pg_status_count(PgCore *core);

/**
 * Fills `out` with the focused pane's status record at `index`.
 */
bool pg_status_at(PgCore *core, size_t index, struct PgStatus *out);

/**
 * Marks a done/error record as seen; it may then be evicted.
 */
bool pg_status_ack(PgCore *core, uint32_t pane_id, uint32_t seq);

/**
 * Splits a pane and returns the **new pane id** (0 on failure).
 */
uint32_t pg_layout_split(PgCore *core, uint32_t pane_id, enum PgAxis axis, float ratio);

/**
 * Closes a pane; its sibling takes over the parent rect.
 */
bool pg_layout_close(PgCore *core, uint32_t pane_id);

/**
 * Changes a split's ratio; clamped so both sides keep their minimum size.
 */
bool pg_layout_resize(PgCore *core, uint32_t split_id, float ratio);

/**
 * One normalized rect per pane, in tree order.
 */
size_t pg_layout_rects(PgCore *core, struct PgRect *out, size_t cap);

/**
 * The split to resize when the user drags the divider next to `pane_id`;
 * 0 when the pane has no parent split.
 */
uint32_t pg_layout_split_of(PgCore *core, uint32_t pane_id);

bool pg_layout_focus(PgCore *core, uint32_t pane_id);

uint32_t pg_layout_focused(PgCore *core);

/**
 * Creates an empty message of `kind`, or NULL for `PG_MSG_UNKNOWN`.
 */
PgMsg *pg_msg_new(enum PgMsgType kind);

void pg_msg_free(PgMsg *msg);

enum PgMsgType pg_msg_type(const PgMsg *msg);

bool pg_msg_set_u32(PgMsg *msg, const char *field, uint32_t value);

bool pg_msg_set_i32(PgMsg *msg, const char *field, int32_t value);

/**
 * Floats are only used for split ratios; they travel as CBOR float32/64.
 */
bool pg_msg_set_f32(PgMsg *msg, const char *field, float value);

float pg_msg_get_f32(const PgMsg *msg, const char *field, float fallback);

bool pg_msg_set_bool(PgMsg *msg, const char *field, bool value);

bool pg_msg_set_str(PgMsg *msg, const char *field, struct PgStr value);

bool pg_msg_set_bytes(PgMsg *msg, const char *field, const uint8_t *data, size_t len);

/**
 * Reads an unsigned field; `fallback` when absent or of the wrong type.
 */
uint32_t pg_msg_get_u32(const PgMsg *msg, const char *field, uint32_t fallback);

int32_t pg_msg_get_i32(const PgMsg *msg, const char *field, int32_t fallback);

bool pg_msg_get_bool(const PgMsg *msg, const char *field, bool fallback);

/**
 * Borrows a text field. The bytes stay valid until `msg` is freed or mutated.
 */
struct PgStr pg_msg_get_str(const PgMsg *msg, const char *field);

/**
 * Bytes needed by [`pg_msg_encode_frame`], or 0 if the message is incomplete.
 */
size_t pg_msg_encoded_len(const PgMsg *msg);

/**
 * Writes one framed message into `out`; returns bytes written, 0 on failure
 * (`PG_ERR_OVERFLOW` when `cap` is too small).
 */
size_t pg_msg_encode_frame(const PgMsg *msg, uint8_t *out, size_t cap);

/**
 * Reads one frame from `buf`.
 *
 * * returns a message and sets `*consumed` — one frame was read;
 * * returns NULL with `*consumed == 0` and `PG_OK` — need more bytes;
 * * returns NULL with an error code — the stream is unusable.
 *
 * An unknown message type yields a `PG_MSG_UNSUPPORTED` handle carrying `t_of`,
 * so the caller can answer without dropping the connection.
 */
PgMsg *pg_msg_decode_frame(const uint8_t *buf, size_t len, size_t *consumed);

/**
 * Builds an owned snapshot from a `snapshot` message; free with
 * [`super::pg_snapshot_free`]. NULL for any other message type.
 *
 * Goes through [`crate::session::snapshot_from_message`], so cell widths (wide
 * chars and their continuation) are derived exactly like everywhere else.
 */
struct PgSnapshot *pg_msg_snapshot(const PgMsg *msg);

/**
 * Payload-only codec, exactly the names the contract asks for: CBOR bytes in and
 * out, without the 4-byte frame prefix.
 */
size_t pg_msg_encode(const PgMsg *msg, uint8_t *out, size_t cap);

/**
 * Decodes one CBOR payload (no length prefix). Unknown `t` yields a handle of
 * type `PG_MSG_UNSUPPORTED` carrying `t_of`.
 */
PgMsg *pg_msg_decode(const uint8_t *payload, size_t len);

/**
 * Rects of a `layout` message, computed by the core (clients never do the maths).
 */
size_t pg_msg_layout_rects(const PgMsg *msg, struct PgRect *out, size_t cap);

/**
 * Dividers of a `layout` message, for divider hit-testing and drawing.
 */
size_t pg_msg_layout_splits(const PgMsg *msg, struct PgSplit *out, size_t cap);

/**
 * Focus carried by a `layout` message (0 when absent).
 */
uint32_t pg_msg_layout_focus(const PgMsg *msg);

/**
 * Sets a field to an array of numbers (e.g. `appearance.ansi`).
 */
bool pg_msg_set_u32_array(PgMsg *msg, const char *field, const uint32_t *values, size_t count);

/**
 * Sets a field to an array of strings (e.g. `hello.features`).
 */
bool pg_msg_set_str_array(PgMsg *msg, const char *field, const struct PgStr *values, size_t count);

/**
 * Number of entries in a `sessions` message.
 */
size_t pg_msg_sessions_count(const PgMsg *msg);

/**
 * Fills `out` with one session entry; returns false when `index` is out of range.
 */
bool pg_msg_session_at(const PgMsg *msg, size_t index, struct PgSessionInfo *out);

/**
 * Copies the pane ids of session `index` into `out`; returns how many were written.
 */
size_t pg_msg_session_panes(const PgMsg *msg, size_t index, uint32_t *out, size_t cap);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* PIGEON_CORE_H */
