#import <AppKit/AppKit.h>
#import <WebKit/WebKit.h>
#import <Network/Network.h>
#import <Network/NSURLSession+Network.h>
#include <arpa/inet.h>
#include <unistd.h>
int main(void) {
 @autoreleasepool {
  NSString *port = [NSString stringWithUTF8String:getenv("FIXTURE_PROXY_PORT")];
  NSString *target = [NSString stringWithUTF8String:getenv("FIXTURE_TARGET_PORT")];
  if (getenv("FIXTURE_CONTROL_POSIX")) {
   int fd=socket(AF_INET,SOCK_STREAM,0); struct sockaddr_in addr={0}; addr.sin_family=AF_INET; addr.sin_port=htons(target.intValue); inet_pton(AF_INET,"127.0.0.1",&addr.sin_addr);
   if(connect(fd,(struct sockaddr *)&addr,sizeof(addr)) != 0) { perror("owned POSIX connect");return 9; }
   const char *request="GET /posix-control HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
   write(fd,request,strlen(request));char bytes[4096];ssize_t count=read(fd,bytes,sizeof(bytes)-1);if(count>0) {bytes[count]=0;printf("POSIX_NATIVE %s\n",bytes);}close(fd);return count>0?0:8;
  }
  nw_endpoint_t endpoint = nw_endpoint_create_host("127.0.0.1", port.UTF8String);
  nw_proxy_config_t proxy = nw_proxy_config_create_socksv5(endpoint);
  nw_proxy_config_set_username_and_password(proxy,"fixture","secret");
  nw_proxy_config_set_failover_allowed(proxy,false);
  if(getenv("FIXTURE_MATCH_HOST")) nw_proxy_config_add_match_domain(proxy, getenv("FIXTURE_MATCH_HOST"));
  NSURLSessionConfiguration *configuration = NSURLSessionConfiguration.ephemeralSessionConfiguration;
  configuration.proxyConfigurations = getenv("FIXTURE_CONTROL_DIRECT") ? @[] : @[proxy];
  configuration.timeoutIntervalForRequest = 10;
  __block BOOL done=NO;
  __block int status=1;
  NSString *host = getenv("FIXTURE_CONTROL_HOST") ? [NSString stringWithUTF8String:getenv("FIXTURE_CONTROL_HOST")] : @"127.0.0.1";
  NSURL *url=[NSURL URLWithString:[NSString stringWithFormat:@"http://%@:%@/objc-control",host,target]];
  NSURLSession *session=[NSURLSession sessionWithConfiguration:configuration];
  [[session dataTaskWithURL:url completionHandler:^(NSData *data,NSURLResponse *response,NSError *error) {
    NSLog(@"CONTROL response=%@ error=%@ body=%@",response,error,[[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding]);
    status = error ? 1 : ([data rangeOfData:[@"remote fixture" dataUsingEncoding:NSUTF8StringEncoding] options:0 range:NSMakeRange(0,data.length)].location != NSNotFound ? 0 : 3); done=YES;
  }] resume];
  NSDate *deadline=[NSDate dateWithTimeIntervalSinceNow:15];
  while(!done && deadline.timeIntervalSinceNow>0) [[NSRunLoop currentRunLoop] runUntilDate:[NSDate dateWithTimeIntervalSinceNow:.01]];
  [session invalidateAndCancel];
  return done ? status : 2;
 }
}
