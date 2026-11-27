# Native descendant supervision: B1 work in progress

The lifecycle contract in `linux_supervisor` is preparatory and does not create a
supervisor, execute a fixture, or establish ownership. Initial child exit cannot
transition it to quiescent. Failure leads to recovery required, and there is no
public transition that asserts quiescence. The one-byte operation decoder rejects
unknown and oversized messages; it is not yet connected to a channel and does
not authenticate an attempt or scope.

The intended design is a private PID namespace with an independently exec'd PID
1, which must adopt and reap orphans. The parent needs an instance-bound private
channel and a reviewed handoff of the already validated executable descriptor.
An authoritative empty child set inside the namespace, after initial status is
collected, is necessary for quiescence. A process group or a subreaper alone is
not sufficient. Cancellation, deadlines, output capture, channel failure, and
namespace teardown remain unimplemented. No native fixtures have been qualified.

The boundary must not be represented as a sandbox against root, kernel
compromise, namespace escape, same-UID interference or ptrace, or processes
created by external brokers. User-namespace capabilities require explicit review.
No raw PID may become durable authority. Future P028 and runtime-view integration
must consume verified quiescence and recovery outcomes; this checkpoint does not
connect them. Existing production execution gates remain in force.
