#ifndef USERAGENT_H
#define USERAGENT_H

#include <QString>

// The identification every request muralis originates carries, GUI included.
// Its one definition is assets/user-agent.tmpl, rendered into MURALIS_USER_AGENT
// at configure time — the same template muralis-core renders for the CLI and
// the Sources, so the two cannot drift apart.
QString muralisUserAgent();

#endif // USERAGENT_H
