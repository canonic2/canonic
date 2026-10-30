# Acme report format

`running` means the service is available. A status read does not start, stop
or repair the service. If a read fails, report that observation rather than
assuming the service is stopped.
