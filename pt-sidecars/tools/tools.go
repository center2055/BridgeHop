//go:build tools

// Pins the pluggable-transport client packages this module builds. The `tools` build constraint is
// never satisfied, so this file is never compiled; `go mod tidy` still follows the imports, which
// keeps go.mod/go.sum complete for every transitive dependency of the sidecars.
//
// Conjure lives in its own nested module (`conjure/`): upstream depends on forked pion/dtls and
// pion/transport modules through `replace` directives, which Go honours only in the main module.
// Declaring them here would swap those forks under Snowflake's WebRTC stack too.
package tools

import (
	_ "gitlab.torproject.org/tpo/anti-censorship/pluggable-transports/lyrebird/cmd/lyrebird"
	_ "gitlab.torproject.org/tpo/anti-censorship/pluggable-transports/snowflake/v2/client"
	_ "gitlab.torproject.org/tpo/anti-censorship/pluggable-transports/webtunnel/main/client"
	_ "www.bamsoftware.com/git/dnstt.git/dnstt-client"
)
