// SPDX-License-Identifier: AGPL-3.0-or-later
// Package repositories on the web (RFC 0031) on macOS: the system's NSURLSession, as JUCE's URL
// streams use it (WebTransport.cpp, elsewhere), but ephemeral: nothing is kept on disk (no HTTP
// cache in ~/Library/Caches, no cookie or HSTS store in ~/Library/HTTPStorages), which a fetch
// has no use for, since every byte fetched is checked by its hash and signature in the core.
#include "WebTransport.h"
#include "oscairn/repository.h"
#import <Foundation/Foundation.h>
#include <condition_variable>
#include <deque>
#include <memory>
#include <mutex>
#include <stdexcept>
#include <string>

namespace
{
    // What the session's delegate hands the fetching thread: the status, the body in chunks (at
    // most 4 MB held: the delegate waits for the reader), and how it ended.
    struct Fetch
    {
        std::mutex mutex;
        std::condition_variable changed;
        std::deque<std::string> chunks;
        size_t held = 0;
        int status = 0, redirects = 0;
        bool done = false, stopped = false;
        std::string error;
    };
}

@interface OscairnFetchDelegate : NSObject <NSURLSessionDataDelegate>
{
@public
    std::shared_ptr<Fetch> fetch;
}
@end

@implementation OscairnFetchDelegate
- (void)URLSession:(NSURLSession*)session dataTask:(NSURLSessionDataTask*)task didReceiveResponse:(NSURLResponse*)response
    completionHandler:(void (^)(NSURLSessionResponseDisposition))handler
{
    {
        std::lock_guard lock(fetch->mutex);
        if([response isKindOfClass:[NSHTTPURLResponse class]]) fetch->status = int(((NSHTTPURLResponse*)response).statusCode);
    }
    handler(NSURLSessionResponseAllow);
}
- (void)URLSession:(NSURLSession*)session dataTask:(NSURLSessionDataTask*)task didReceiveData:(NSData*)data
{
    std::unique_lock lock(fetch->mutex);
    fetch->changed.wait(lock, [&] { return fetch->held < (4u << 20) || fetch->stopped; });
    if(fetch->stopped) return;
    [data enumerateByteRangesUsingBlock:^(const void* bytes, NSRange range, BOOL*) {
        fetch->chunks.emplace_back(static_cast<const char*>(bytes), range.length);
        fetch->held += range.length;
    }];
    fetch->changed.notify_all();
}
// As many redirects as JUCE's streams were given (release downloads redirect to another host).
- (void)URLSession:(NSURLSession*)session task:(NSURLSessionTask*)task willPerformHTTPRedirection:(NSHTTPURLResponse*)response
    newRequest:(NSURLRequest*)request completionHandler:(void (^)(NSURLRequest*))handler
{
    std::unique_lock lock(fetch->mutex);
    const bool follow = ++fetch->redirects <= 5;
    lock.unlock();
    handler(follow ? request : nil);
}
- (void)URLSession:(NSURLSession*)session task:(NSURLSessionTask*)task didCompleteWithError:(NSError*)error
{
    std::lock_guard lock(fetch->mutex);
    fetch->done = true;
    if(error) fetch->error = error.localizedDescription.UTF8String;
    fetch->changed.notify_all();
}
@end

namespace oscairn::host
{
    void useWebTransport()
    {
        repository::setTransport([](const std::string& url, uint64_t from, const std::function<bool(int, const char*, size_t)>& sink) {
            @autoreleasepool
            {
                NSURL* address = [NSURL URLWithString:[NSString stringWithUTF8String:url.c_str()]];
                if(!address) throw std::runtime_error("cannot reach " + url);
                NSMutableURLRequest* request = [NSMutableURLRequest requestWithURL:address cachePolicy:NSURLRequestReloadIgnoringLocalCacheData timeoutInterval:15];
                if(from) [request setValue:[NSString stringWithFormat:@"bytes=%llu-", (unsigned long long)from] forHTTPHeaderField:@"Range"];
                auto fetch = std::make_shared<Fetch>();
                OscairnFetchDelegate* delegate = [OscairnFetchDelegate new];
                delegate->fetch = fetch;
                NSURLSession* session = [NSURLSession sessionWithConfiguration:[NSURLSessionConfiguration ephemeralSessionConfiguration] delegate:delegate delegateQueue:nil];
                [[session dataTaskWithRequest:request] resume];
                // However this ends (the sink had enough, or threw), the delegate waits no longer and the session goes.
                struct Done
                {
                    Fetch& fetch; NSURLSession* session;
                    ~Done()
                    {
                        { std::lock_guard lock(fetch.mutex); fetch.stopped = true; }
                        fetch.changed.notify_all();
                        [session invalidateAndCancel]; // it lets go of the delegate
                    }
                } done{*fetch, session};
                for(;;)
                {
                    std::string chunk;
                    int status = 0;
                    {
                        std::unique_lock lock(fetch->mutex);
                        fetch->changed.wait(lock, [&] { return !fetch->chunks.empty() || fetch->done; });
                        if(fetch->chunks.empty()) break;
                        chunk = std::move(fetch->chunks.front());
                        fetch->chunks.pop_front();
                        fetch->held -= chunk.size();
                        status = fetch->status;
                    }
                    fetch->changed.notify_all();
                    if(!sink(status, chunk.data(), chunk.size())) break;
                }
                std::lock_guard lock(fetch->mutex);
                // No answer at all (refused, a certificate not trusted): as JUCE's stream, none.
                if(!fetch->status) throw std::runtime_error("cannot reach " + url + (fetch->error.empty() ? std::string() : ": " + fetch->error));
                return fetch->status;
            }
        });
    }
}
