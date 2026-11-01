#include <mpv/client.h>
#include <mpv/render.h>
#include <mpv/render_gl.h>
#include <OpenGL/gl3.h>
#include <OpenGL/CGLIOSurface.h>
#include <OpenGL/OpenGL.h>
#include <IOSurface/IOSurface.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <dlfcn.h>
#import <Metal/Metal.h>

static double now_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1000.0 + ts.tv_nsec / 1e6;
}

static int errors = 0;

static void *get_proc_address(void *context, const char *name) {
    (void)context;
    return dlsym(RTLD_DEFAULT, name);
}

#define CHECK(cond, label)                                  \
    do {                                                    \
        if (!(cond)) {                                      \
            fprintf(stderr, "FAIL %s\n", label);            \
            errors++;                                       \
        } else {                                            \
            fprintf(stderr, "ok   %s\n", label);            \
        }                                                   \
    } while (0)

int main(int argc, char **argv) {
    if (argc < 2) {
        fprintf(stderr, "usage: probe <file> [seconds]\n");
        return 2;
    }
    const char *path = argv[1];
    double seconds = argc > 2 ? atof(argv[2]) : 6.0;
    int untimed = argc > 3 && strcmp(argv[3], "untimed") == 0;

    CGLPixelFormatAttribute attributes[] = {
        kCGLPFAOpenGLProfile, (CGLPixelFormatAttribute)kCGLOGLPVersion_3_2_Core,
        kCGLPFAAccelerated,
        kCGLPFADoubleBuffer,
        (CGLPixelFormatAttribute)0,
    };
    CGLPixelFormatObj pixel_format = NULL;
    GLint count = 0;
    CGLError error = CGLChoosePixelFormat(attributes, &pixel_format, &count);
    CHECK(error == kCGLNoError && pixel_format != NULL, "CGL pixel format");
    if (!pixel_format) return 1;

    CGLContextObj context = NULL;
    error = CGLCreateContext(pixel_format, NULL, &context);
    CHECK(error == kCGLNoError && context != NULL, "CGL context");
    if (!context) return 1;
    CGLSetCurrentContext(context);

    GLint major = 0, minor = 0;
    glGetIntegerv(GL_MAJOR_VERSION, &major);
    glGetIntegerv(GL_MINOR_VERSION, &minor);
    fprintf(stderr, "GL %d.%d\n", major, minor);
    const GLubyte *renderer = glGetString(GL_RENDERER);
    fprintf(stderr, "GL_RENDERER %s\n", renderer ? (const char *)renderer : "?");

    const int width = 3840, height = 2160;
    CFMutableDictionaryRef props = CFDictionaryCreateMutable(
        NULL, 4, &kCFTypeDictionaryKeyCallBacks, &kCFTypeDictionaryValueCallBacks);
    long bytes_per_element = 4;
    long bytes_per_row = width * 4;
    CFNumberRef number = CFNumberCreate(NULL, kCFNumberLongType, &bytes_per_element);
    CFDictionarySetValue(props, kIOSurfaceBytesPerElement, number);
    CFRelease(number);
    number = CFNumberCreate(NULL, kCFNumberLongType, &bytes_per_row);
    CFDictionarySetValue(props, kIOSurfaceBytesPerRow, number);
    CFRelease(number);
    long w = width, h = height;
    number = CFNumberCreate(NULL, kCFNumberLongType, &w);
    CFDictionarySetValue(props, kIOSurfaceWidth, number);
    CFRelease(number);
    number = CFNumberCreate(NULL, kCFNumberLongType, &h);
    CFDictionarySetValue(props, kIOSurfaceHeight, number);
    CFRelease(number);
    int pixel_format_int = 'BGRA';
    number = CFNumberCreate(NULL, kCFNumberIntType, &pixel_format_int);
    CFDictionarySetValue(props, kIOSurfacePixelFormat, number);
    CFRelease(number);

    IOSurfaceRef surface = IOSurfaceCreate(props);
    CFRelease(props);
    CHECK(surface != NULL, "IOSurface created");
    if (!surface) return 1;
    CHECK(IOSurfaceGetBytesPerRow(surface) == (size_t)bytes_per_row, "row stride as requested");
    CHECK(IOSurfaceGetWidth(surface) == (size_t)width && IOSurfaceGetHeight(surface) == (size_t)height,
          "surface size");

    GLuint texture = 0;
    glGenTextures(1, &texture);
    glBindTexture(GL_TEXTURE_RECTANGLE, texture);
    CGLError texture_error = CGLTexImageIOSurface2D(context, GL_TEXTURE_RECTANGLE, GL_RGBA8,
                                                    width, height, GL_BGRA, GL_UNSIGNED_INT_8_8_8_8_REV,
                                                    surface, 0);
    CHECK(texture_error == kCGLNoError, "CGLTexImageIOSurface2D");

    GLuint framebuffer = 0;
    glGenFramebuffers(1, &framebuffer);
    glBindFramebuffer(GL_FRAMEBUFFER, framebuffer);
    glFramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_RECTANGLE, texture, 0);
    CHECK(glCheckFramebufferStatus(GL_FRAMEBUFFER) == GL_FRAMEBUFFER_COMPLETE, "framebuffer complete");

    mpv_handle *mpv = mpv_create();
    CHECK(mpv != NULL, "mpv_create");
    mpv_set_option_string(mpv, "vo", "libmpv");
    mpv_set_option_string(mpv, "idle", "yes");
    mpv_set_option_string(mpv, "config", "no");
    mpv_set_option_string(mpv, "terminal", "no");
    mpv_set_option_string(mpv, "aid", "no");
    mpv_set_option_string(mpv, "ao", "null");
    mpv_set_option_string(mpv, "hwdec", "auto-copy");
    mpv_set_option_string(mpv, "demuxer-max-bytes", "512MiB");
    mpv_set_option_string(mpv, "loop-file", "inf");
    if (untimed) mpv_set_option_string(mpv, "untimed", "yes");
    CHECK(mpv_initialize(mpv) >= 0, "mpv_initialize");

    mpv_opengl_init_params gl_init = {
        .get_proc_address = get_proc_address,
        .get_proc_address_ctx = NULL,
    };

    int advanced_control = 1;
    mpv_render_param params[] = {
        {MPV_RENDER_PARAM_API_TYPE, (void *)MPV_RENDER_API_TYPE_OPENGL},
        {MPV_RENDER_PARAM_OPENGL_INIT_PARAMS, &gl_init},
        {MPV_RENDER_PARAM_ADVANCED_CONTROL, &advanced_control},
        {MPV_RENDER_PARAM_INVALID, NULL},
    };
    mpv_render_context *render = NULL;
    int status = mpv_render_context_create(&render, mpv, params);
    fprintf(stderr, "mpv_render_context_create status=%d (%s)\n", status, mpv_error_string(status));
    CHECK(status >= 0, "openGL render context");
    if (status < 0) return 1;

    mpv_render_param fbo_params[] = {
        {MPV_RENDER_PARAM_OPENGL_FBO, &(mpv_opengl_fbo){.fbo = (int)framebuffer, .w = width, .h = height}},
        {MPV_RENDER_PARAM_FLIP_Y, &(int){1}},
        {MPV_RENDER_PARAM_INVALID, NULL},
    };

    const char *cmd[] = {"loadfile", path, NULL};
    mpv_command(mpv, cmd);

    int rendered = 0;
    double render_total = 0.0, worst = 0.0;
    double began = now_ms();
    double last = began;
    double deadline = began + seconds * 1000.0;
    while (now_ms() < deadline) {
        mpv_event *event = mpv_wait_event(mpv, 0);
        if (event && event->event_id == MPV_EVENT_SHUTDOWN) break;
        if (event && event->event_id == MPV_EVENT_END_FILE) break;
        if (event && (event->event_id == MPV_EVENT_VIDEO_RECONFIG || event->event_id == MPV_EVENT_FILE_LOADED)) {
            fprintf(stderr, "event %s\n", mpv_event_name(event->event_id));
        }
        if (mpv_render_context_update(render) & MPV_RENDER_UPDATE_FRAME) {
            double start = now_ms();
            glBindFramebuffer(GL_FRAMEBUFFER, framebuffer);
            int r = mpv_render_context_render(render, fbo_params);
            glFinish();
            double took = now_ms() - start;
            if (r < 0) {
                fprintf(stderr, "render failed: %s\n", mpv_error_string(r));
                break;
            }
            rendered++;
            render_total += took;
            if (took > worst) worst = took;
            last = now_ms();
        } else {
            struct timespec pause = {0, 2000000};
            nanosleep(&pause, NULL);
        }
    }
    double elapsed = (now_ms() - began) / 1000.0;
    fprintf(stderr, "rendered %d frames in %.2fs = %.2f fps, render mean %.2f ms, worst %.2f ms\n",
            rendered, elapsed, rendered / elapsed, rendered ? render_total / rendered : 0.0, worst);

    IOSurfaceLock(surface, kIOSurfaceLockReadOnly, NULL);
    unsigned char *base = (unsigned char *)IOSurfaceGetBaseAddress(surface);
    size_t stride = IOSurfaceGetBytesPerRow(surface);
    long nonblack = 0, sampled = 0;
    for (int y = 100; y < height; y += 97) {
        for (int x = 100; x < width; x += 97) {
            unsigned char *pixel = base + (size_t)y * stride + (size_t)x * 4;
            sampled++;
            if (pixel[0] || pixel[1] || pixel[2]) nonblack++;
        }
    }
    unsigned char *first = base + (size_t)(height / 2) * stride + (size_t)(width / 2) * 4;
    fprintf(stderr, "centre pixel B=%u G=%u R=%u A=%u; %ld of %ld sampled pixels non-black\n",
            first[0], first[1], first[2], first[3], nonblack, sampled);
    IOSurfaceUnlock(surface, kIOSurfaceLockReadOnly, NULL);
    CHECK(rendered > 0, "frames rendered through OpenGL");
    CHECK(nonblack * 2 > sampled, "the IOSurface holds a picture");

    id<MTLDevice> metal_device = MTLCreateSystemDefaultDevice();
    CHECK(metal_device != nil, "Metal device");
    MTLTextureDescriptor *descriptor = [MTLTextureDescriptor
        texture2DDescriptorWithPixelFormat:MTLPixelFormatBGRA8Unorm
                                     width:width
                                    height:height
                                 mipmapped:NO];
    descriptor.usage = MTLTextureUsageShaderRead;
    descriptor.storageMode = MTLStorageModeShared;
    id<MTLTexture> metal_texture = [metal_device newTextureWithDescriptor:descriptor
                                                               iosurface:surface
                                                                   plane:0];
    CHECK(metal_texture != nil, "Metal texture wraps the IOSurface");
    if (metal_texture) {
        unsigned char sample[64];
        memset(sample, 0, sizeof(sample));
        [metal_texture getBytes:sample
                   bytesPerRow:16 * 4
                    fromRegion:MTLRegionMake2D(width / 2, height / 2, 16, 1)
                   mipmapLevel:0];
        unsigned char *surface_pixel = base + (size_t)(height / 2) * stride + (size_t)(width / 2) * 4;
        int same = memcmp(sample, surface_pixel, 16 * 4) == 0;
        fprintf(stderr, "metal read B=%u G=%u R=%u A=%u, surface B=%u G=%u R=%u A=%u, identical=%d\n",
                sample[0], sample[1], sample[2], sample[3],
                surface_pixel[0], surface_pixel[1], surface_pixel[2], surface_pixel[3], same);
        CHECK(same, "Metal sees the pixels OpenGL wrote");
        CHECK(sample[0] || sample[1] || sample[2], "and they are a picture, not black");
    }

    mpv_render_context_free(render);
    mpv_terminate_destroy(mpv);
    glDeleteFramebuffers(1, &framebuffer);
    glDeleteTextures(1, &texture);
    CFRelease(surface);
    CGLSetCurrentContext(NULL);
    CGLDestroyContext(context);
    CGLDestroyPixelFormat(pixel_format);
    fprintf(stderr, "%s\n", errors ? "PROBE FAILED" : "PROBE PASSED");
    return errors ? 1 : 0;
}
