# Problems API

> [!information]
> All endpoints have a unified prefix: `/problems`.
>
> There is successful responses only.

## API

| Endpoint      | Method | Request                      | Response                         | Description                                   |
| ------------- | ------ | ---------------------------- | -------------------------------- | --------------------------------------------- |
| `/`           | GET    | [[list-problems-queries]]    | [[list-problems-response]]       | List problems                                 |
| `/`           | POST   | [[post-problem-request]]     | [[post-problem-response]]        | Post a problem without pushing test cases     |
| `/stat`       | GET    | None                         | [[get-problems-state-response]]  | Get global api stat                           |
| `/:id`        | GET    | None                         | [[get-problem-details-response]] | Get specific problem details                  |
| `/:id`        | PATCH  | [[update-problem-request]]   | 204                              | Update specific problem (author/admin only)   |
| `/:id`        | DELETE | None                         | 204                              | Delete a problem (author/admin only)          |
| `/:id/cases`  | GET    | None                         | [[get-test-cases-response]]      | Get test cases (will return insensitive data) |
| `/:id/cases`  | PUT    | [[update-test-case-request]] | 204                              | Overwrite test cases (author/admin only)      |

## Models

### List problems queries

```plain
{
    limit: optional[integer];
    page: optional[integer]; // Start from 0
    query: optional[string]; // Title only
    difficulty: optional[enum[difficulty]];
    tag: optional[array[uuid]]; // And
}
```

### List problems response

```plain
{
    problems: array[{
        id: uuid;
        title: String;
        difficulty: enum[difficulty];
        tags: array[uuid];
    }];
    total: integer;
}
```

### Get problems stat response

```plain
{
    total: integer;
}
```

### Get test cases response

> [!information]
> Author can review hidden cases

```plain
array[{
    id: uuid;
    input: string;
    output: string;
    type: enum[testCaseType];
}]
```

### Get problem details response

```plain
{
    id: uuid;
    authorId: uuid;
    title: string;
    description: string;
    difficulty: enum[difficulty];
    tags: array[uuid];
    createdAt: timestamp;
    updatedAt: timestamp;
    limit: {
        cpuTimeMs: integer;
        wallTimeMs: integer;
        memoryBytes: integer;
        outputBytes: integer;
    };
}
```

### Update problem request

```plain
{
    title: optional[string];
    description: optional[string];
    difficulty: optional[enum[difficulty]];
    limit: optional[{
        cpuTimeMs: optional[integer];
        wallTimeMs: optional[integer];
        memoryBytes: optional[integer];
        outputBytes: optional[integer];
    }];
    tags: optional[array[uuid]];
}
```

### Update test case request

```plain
{
    cases: array[{
        input: string;
        output: string;
        type: enum[testCaseType];
    }];
}
```

### Post problem request

```plain
{
    title: string;
    description: string;
    difficulty: enum[difficulty];
    tags: array[uuid];
    limit: {
        cpuTimeMs: integer;
        wallTimeMs: integer;
        memoryBytes: integer;
        outputBytes: integer;
    };
}
```

### Post problem response

```plain
{
    id: uuid;
}
```
