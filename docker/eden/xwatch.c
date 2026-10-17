// xwatch: reads the X screen with XShmGetImage as fast as it can and reports, per second, how many grabs it
// took, how many distinct pictures it saw (what the program shows per second, up to the grab rate), how many
// grabs had a black block, and the mean colour of probe areas. Every grab with a black block is logged with
// its time and the block's bounds, and the first ones are saved as PPM files.
//
//   xwatch [-t seconds] [-a x,y,w,h] [-p x,y,w,h]... [-s dir] [-n max_saved]
//
// -a  area that is checked (default: the whole screen)
// -p  probe area whose mean RGB is printed every second (repeatable)
// A black block is a 16x16 cell of the checked area, aligned to it, whose pixels are all exactly (0,0,0).
#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <X11/extensions/XShm.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ipc.h>
#include <sys/shm.h>
#include <time.h>

#define CELL 16
#define MAX_PROBES 8

struct rect { int x, y, w, h; };

static double now(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec + t.tv_nsec / 1e9;
}

static int parse_rect(const char *s, struct rect *r) {
    return sscanf(s, "%d,%d,%d,%d", &r->x, &r->y, &r->w, &r->h) == 4;
}

static void save(const char *dir, int index, double t, XImage *img) {
    char path[512];
    snprintf(path, sizeof(path), "%s/black-%03d-%.3fs.ppm", dir, index, t);
    FILE *f = fopen(path, "wb");
    if (!f) return;
    fprintf(f, "P6\n%d %d\n255\n", img->width, img->height);
    for (int y = 0; y < img->height; y++) {
        const uint32_t *row = (const uint32_t *)(img->data + (size_t)y * img->bytes_per_line);
        for (int x = 0; x < img->width; x++) {
            unsigned char px[3] = {(row[x] >> 16) & 255, (row[x] >> 8) & 255, row[x] & 255};
            fwrite(px, 1, 3, f);
        }
    }
    fclose(f);
}

int main(int argc, char **argv) {
    double seconds = 60;
    struct rect area = {0, 0, 0, 0}, probes[MAX_PROBES];
    int nprobes = 0, max_saved = 20;
    const char *dir = NULL;
    for (int i = 1; i + 1 < argc; i += 2) {
        if (!strcmp(argv[i], "-t")) seconds = atof(argv[i + 1]);
        else if (!strcmp(argv[i], "-a")) parse_rect(argv[i + 1], &area);
        else if (!strcmp(argv[i], "-p") && nprobes < MAX_PROBES) parse_rect(argv[i + 1], &probes[nprobes++]);
        else if (!strcmp(argv[i], "-s")) dir = argv[i + 1];
        else if (!strcmp(argv[i], "-n")) max_saved = atoi(argv[i + 1]);
    }
    Display *dpy = XOpenDisplay(NULL);
    if (!dpy) {
        fprintf(stderr, "xwatch: cannot open display\n");
        return 2;
    }
    int screen = DefaultScreen(dpy);
    Window root = RootWindow(dpy, screen);
    int w = DisplayWidth(dpy, screen), h = DisplayHeight(dpy, screen);
    if (area.w <= 0 || area.h <= 0) area = (struct rect){0, 0, w, h};
    if (!XShmQueryExtension(dpy)) {
        fprintf(stderr, "xwatch: no MIT-SHM\n");
        return 2;
    }
    XShmSegmentInfo shm;
    XImage *img = XShmCreateImage(dpy, DefaultVisual(dpy, screen), DefaultDepth(dpy, screen), ZPixmap, NULL, &shm,
                                  w, h);
    shm.shmid = shmget(IPC_PRIVATE, (size_t)img->bytes_per_line * img->height, IPC_CREAT | 0600);
    shm.shmaddr = img->data = shmat(shm.shmid, NULL, 0);
    shm.readOnly = False;
    XShmAttach(dpy, &shm);
    XSync(dpy, False);
    shmctl(shm.shmid, IPC_RMID, NULL);

    const int cw = area.w / CELL, ch = area.h / CELL;
    uint64_t last_hash = 0;
    long grabs = 0, distinct = 0, black = 0, saved = 0;
    long sec_grabs = 0, sec_distinct = 0, sec_black = 0;
    double sum[MAX_PROBES][3] = {{0}};
    long probe_n[MAX_PROBES] = {0};
    const double start = now();
    double second = start;
    printf("xwatch: %dx%d, area %d,%d %dx%d, cell %d\n", w, h, area.x, area.y, area.w, area.h, CELL);
    for (;;) {
        const double t = now();
        if (t - start >= seconds) break;
        XShmGetImage(dpy, root, img, 0, 0, AllPlanes);
        grabs++, sec_grabs++;

        // A picture is new when a sparse sample of it changed.
        uint64_t hash = 1469598103934665603ull;
        for (int y = area.y; y < area.y + area.h; y += 7) {
            const uint32_t *row = (const uint32_t *)(img->data + (size_t)y * img->bytes_per_line);
            for (int x = area.x; x < area.x + area.w; x += 13) hash = (hash ^ (row[x] & 0xffffff)) * 1099511628211ull;
        }
        const int is_new = hash != last_hash;
        last_hash = hash;
        if (is_new) distinct++, sec_distinct++;

        int cells = 0, bx0 = 1 << 30, by0 = 1 << 30, bx1 = -1, by1 = -1;
        if (is_new) {
            for (int cy = 0; cy < ch; cy++) {
                for (int cx = 0; cx < cw; cx++) {
                    int all = 1;
                    for (int y = 0; y < CELL && all; y++) {
                        const uint32_t *row = (const uint32_t *)(img->data +
                            (size_t)(area.y + cy * CELL + y) * img->bytes_per_line) + area.x + cx * CELL;
                        for (int x = 0; x < CELL; x++)
                            if (row[x] & 0xffffff) { all = 0; break; }
                    }
                    if (all) {
                        cells++;
                        if (cx < bx0) bx0 = cx;
                        if (cy < by0) by0 = cy;
                        if (cx > bx1) bx1 = cx;
                        if (cy > by1) by1 = cy;
                    }
                }
            }
        }
        if (cells > 0) {
            black++, sec_black++;
            printf("BLACK mono=%.4f t=%.3fs cells=%d (%.1f%%) bounds x=%d..%d y=%d..%d\n", t, t - start, cells,
                   100.0 * cells / (cw * ch), area.x + bx0 * CELL, area.x + (bx1 + 1) * CELL,
                   area.y + by0 * CELL, area.y + (by1 + 1) * CELL);
            if (dir && saved < max_saved) save(dir, (int)saved++, t - start, img);
        }
        for (int p = 0; p < nprobes; p++) {
            for (int y = probes[p].y; y < probes[p].y + probes[p].h; y++) {
                const uint32_t *row = (const uint32_t *)(img->data + (size_t)y * img->bytes_per_line);
                for (int x = probes[p].x; x < probes[p].x + probes[p].w; x++) {
                    sum[p][0] += (row[x] >> 16) & 255, sum[p][1] += (row[x] >> 8) & 255, sum[p][2] += row[x] & 255;
                    probe_n[p]++;
                }
            }
        }
        if (t - second >= 1.0) {
            printf("SEC t=%.0fs grabs=%ld pictures=%ld black=%ld", t - start, sec_grabs, sec_distinct, sec_black);
            for (int p = 0; p < nprobes; p++) {
                if (probe_n[p])
                    printf(" probe%d=%.0f,%.0f,%.0f", p, sum[p][0] / probe_n[p], sum[p][1] / probe_n[p],
                           sum[p][2] / probe_n[p]);
                sum[p][0] = sum[p][1] = sum[p][2] = 0, probe_n[p] = 0;
            }
            printf("\n");
            fflush(stdout);
            sec_grabs = sec_distinct = sec_black = 0;
            second = t;
        }
    }
    const double span = now() - start;
    printf("TOTAL seconds=%.1f grabs=%ld (%.0f/s) pictures=%ld (%.1f/s) black=%ld (%.2f%% of pictures)\n", span,
           grabs, grabs / span, distinct, distinct / span, black, distinct ? 100.0 * black / distinct : 0.0);
    return 0;
}
