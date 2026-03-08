# API Reference

Complete reference documentation for gRPC Testify.

## Overview

gRPC Testify provides a comprehensive testing framework for gRPC services using configuration files. This section documents all available features, syntax, and configuration options.

## Reference Sections

### [Command Line Interface](./command-line)
Complete documentation of all command-line options, flags, and usage patterns.

### [Test File Format](./test-files)  
Detailed specification of the `.gctf` test file format, including all sections and syntax.

### [Assertions & Validation](./assertions)
Comprehensive guide to assertion syntax and validation patterns.

### [Plugin System](../../plugins/)
Built-in assertion functions available in `ASSERTS`.

### [Report Formats](./report-formats)
Complete guide to output formats: console and JUnit XML reports.

### [Type Validation](./type-validation)
Advanced type validators for UUID, timestamps, URLs, emails, and more specialized data types.

### [Plugin Development](./plugin-development)
Contributor notes for built-in plugin modules.

## Quick Reference

### Basic Test File Structure
```php
--- ADDRESS ---
localhost:4770

--- ENDPOINT ---
service.Method

--- REQUEST ---
{
  "field": "value"
}

--- RESPONSE ---
{
  "result": "*"
}

--- ASSERTS ---
.result | length > 0
```

### Common Command Usage
```bash
# Run single test
grpctestify test.gctf

# Run all tests in directory
grpctestify tests/

# Run with options
grpctestify --parallel 4 --verbose tests/
```

## See Also

- [Getting Started Guide](../../getting-started/installation)
- [Examples](../../examples/)
