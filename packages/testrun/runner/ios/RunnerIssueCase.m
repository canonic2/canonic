#import "RunnerIssueCase.h"

@implementation SBRunnerIssueCase {
    SBInputIssueTracker *_inputIssues;
}
- (SBInputIssueTracker *)inputIssues {
    @synchronized(self) {
        if (!_inputIssues)
            _inputIssues = [SBInputIssueTracker new];
        return _inputIssues;
    }
}
- (void)recordIssue:(XCTIssue *)issue {
    [self.inputIssues record];
    [super recordIssue:issue];
}
@end
