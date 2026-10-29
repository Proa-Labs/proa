# Fixture escaping & Unicode

Synthetic text for client tests: café, 日本語, `?q=a&b`, and <example>.
JSON clients should recover this Markdown without double-escaping it.
