import { Language } from "models/judge-core";

export const difficultyEnumLiteral = ["入门", "普及-", "普及/提高-", "普及+/提高", "提高+/省选-", "省选/NOI-", "NOI/NOI+/CTSC"] as const;
export const testCaseTypeEnumLiteral = ["example", "hidden"] as const;
export const submissionStatusEnumLiteral = ["pending", "completed"] as const;
export const acceptableLanguageEnumLiteral = [Language.Cpp] as const;

export type Difficulty = (typeof difficultyEnumLiteral)[number];
export type TestCaseType = (typeof testCaseTypeEnumLiteral)[number];
export type SubmissionStatus = (typeof submissionStatusEnumLiteral)[number];
export type AcceptableLanguage = (typeof acceptableLanguageEnumLiteral)[number];
