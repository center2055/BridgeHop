//go:build tools

// Same pinning trick as ../tools/tools.go, for the Conjure client, which needs its own module
// because of upstream `replace` directives (see ../tools/tools.go).
package tools

import (
	_ "gitlab.torproject.org/tpo/anti-censorship/pluggable-transports/conjure/client"
)
