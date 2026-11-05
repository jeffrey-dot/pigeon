/* Runtime compatibility with the original PgCell/PgSnapshot prefix. */
#include "pigeon_core.h"
#include <assert.h>
#include <stddef.h>
#include <string.h>

struct OldCell {
    uint32_t size, codepoint, fg, bg, attrs, hyperlink_id;
    uint8_t width;
};
struct OldRow { uint32_t size; const struct OldCell *cells; };
struct OldSnapshot {
    uint32_t size, version, pane_id;
    uint16_t cols, rows;
    uint8_t alt_screen;
    struct PgCursor cursor;
    struct PgScrollRegion scroll_region;
    const struct OldRow *screen, *history;
    uint32_t history_count;
};
_Static_assert(sizeof(struct OldCell) == sizeof(PgCell), "legacy cell stride");
_Static_assert(offsetof(struct OldSnapshot, screen) == offsetof(PgSnapshot, screen), "legacy snapshot prefix");

int main(void) {
    PgCore *core = pg_core_new(NULL);
    assert(core);
    uint32_t pane = pg_pane_new(core, NULL);
    const char input[] = "\033]8;;https://example.com\007hi\033]8;;\007!";
    assert(pg_pane_feed(core, pane, (const uint8_t *)input, sizeof(input) - 1));
    PgSnapshot *snapshot = pg_pane_snapshot(core, pane);
    assert(snapshot);
    pg_core_free(core);
    struct OldSnapshot old;
    memcpy(&old, snapshot, sizeof(old));
    struct OldRow row;
    memcpy(&row, old.screen, sizeof(row));
    struct OldCell cells[3];
    memcpy(cells, row.cells, sizeof(cells));
    assert(cells[0].codepoint == 'h' && cells[1].codepoint == 'i' && cells[2].codepoint == '!');
    assert(cells[0].hyperlink_id == cells[1].hyperlink_id && cells[2].hyperlink_id == 0);
    assert(snapshot->link_count == 1 && snapshot->links[0].id == cells[0].hyperlink_id);
    const PgStr uri = snapshot->links[0].uri;
    assert(uri.len == strlen("https://example.com"));
    assert(memcmp(uri.ptr, "https://example.com", uri.len) == 0);
    pg_snapshot_free(snapshot);
    return 0;
}
