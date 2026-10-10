#ifndef SILIQ_NATIVE_H
#define SILIQ_NATIVE_H
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct SiliqNativeHandle SiliqNativeHandle;
typedef struct SiliqNativeBuffer {
    uint8_t *data;
    size_t len;
} SiliqNativeBuffer;

/* UTF-8 NUL-terminated absolute paths, max 4095 bytes each. Returns NULL on
 * failure. The creator's thread can read last_error() for the JSON envelope.
 * No WebView or process/server is initialized by this interface. */
SiliqNativeHandle *siliq_native_create(const char *workspace, const char *fixtures);

/* request: {"command":"...","args":{...}}, maximum 20 MiB.
 * response: {"ok":true,"value":...} or {"ok":false,"issues":[...]},
 * maximum 64 MiB. Caller owns the buffer and must free it exactly once.
 * Requests are thread safe; handle close may overlap a pending request. */
SiliqNativeBuffer siliq_native_request(SiliqNativeHandle *handle, const uint8_t *request, size_t length);
SiliqNativeBuffer siliq_native_last_error(void);
void siliq_native_free(SiliqNativeBuffer buffer);
void siliq_native_close(SiliqNativeHandle *handle);

#ifdef __cplusplus
}
#endif
#endif
