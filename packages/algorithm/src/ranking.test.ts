import { describe, expect, it } from "bun:test";
import {
	type IIcpcEntry,
	type IIcpcOptions,
	icpcAssignRanks,
	icpcPenalty,
	icpcRankSort,
} from "./ranking";

const defaultOptions: IIcpcOptions = { penaltySeconds: 1200 };

describe("icpcPenalty", () => {
	it("returns `timeUsing` when there are no penalty counts", () => {
		const entry: IIcpcEntry = {
			id: "team-1",
			solvedProblems: 3,
			penaltyCounts: 0,
			timeUsing: 5400,
		};

		expect(icpcPenalty(entry, defaultOptions)).toBe(5400);
	});

	it("adds penalty seconds multiplied by penalty counts", () => {
		const entry: IIcpcEntry = {
			id: "team-1",
			solvedProblems: 2,
			penaltyCounts: 3,
			timeUsing: 3600,
		};

		// 3600 + 3 * 1200 = 7200
		expect(icpcPenalty(entry, defaultOptions)).toBe(7200);
	});

	it("handles zero `timeUsing` with penalties", () => {
		const entry: IIcpcEntry = {
			id: "team-new",
			solvedProblems: 0,
			penaltyCounts: 5,
			timeUsing: 0,
		};

		expect(icpcPenalty(entry, defaultOptions)).toBe(6000);
	});

	it("handles zero `penaltySeconds`", () => {
		const entry: IIcpcEntry = {
			id: "team-1",
			solvedProblems: 1,
			penaltyCounts: 4,
			timeUsing: 100,
		};

		expect(icpcPenalty(entry, { penaltySeconds: 0 })).toBe(100);
	});
});

describe("icpcRankSort", () => {
	const teams: IIcpcEntry[] = [
		{ id: "A", solvedProblems: 3, penaltyCounts: 2, timeUsing: 3000 },
		{ id: "B", solvedProblems: 5, penaltyCounts: 1, timeUsing: 4000 },
		{ id: "C", solvedProblems: 3, penaltyCounts: 0, timeUsing: 4500 },
		{ id: "D", solvedProblems: 5, penaltyCounts: 3, timeUsing: 3500 },
		{ id: "E", solvedProblems: 1, penaltyCounts: 0, timeUsing: 1000 },
	];

	it("sorts by `solvedProblems` descending first", () => {
		const sorted = icpcRankSort(teams, defaultOptions);
		const problems = sorted.map((t) => t.solvedProblems);

		expect(problems).toEqual([5, 5, 3, 3, 1]);
	});

	it("sorts by penalty ascending within same `solvedProblems`", () => {
		const sorted = icpcRankSort(teams, defaultOptions);
		const topGroup = sorted.filter((t) => t.solvedProblems === 5);

		// D: 3500 + 3*1200 = 7100, B: 4000 + 1*1200 = 5200 → B < D
		expect(topGroup.map((t) => t.id)).toEqual(["B", "D"]);
	});

	it("breaks ties by penalty within same problem count", () => {
		const sorted = icpcRankSort(teams, defaultOptions);
		const midGroup = sorted.filter((t) => t.solvedProblems === 3);

		// A: 3000 + 2*1200 = 5400, C: 4500 + 0*1200 = 4500 → C < A
		expect(midGroup.map((t) => t.id)).toEqual(["C", "A"]);
	});

	it("places team with fewest solved at end", () => {
		const sorted = icpcRankSort(teams, defaultOptions);

		expect(sorted.at(-1)!.id).toBe("E");
	});

	it("does not mutate the original array", () => {
		const original = [...teams];
		icpcRankSort(teams, defaultOptions);

		expect(teams).toEqual(original);
	});

	it("returns a new array instance", () => {
		const sorted = icpcRankSort(teams, defaultOptions);

		expect(sorted).not.toBe(teams);
	});

	it("handles empty array", () => {
		const sorted = icpcRankSort([], defaultOptions);

		expect(sorted).toEqual([]);
	});

	it("handles single entry", () => {
		const entry: IIcpcEntry = {
			id: "solo",
			solvedProblems: 2,
			penaltyCounts: 1,
			timeUsing: 600,
		};

		expect(icpcRankSort([entry], defaultOptions)).toEqual([entry]);
	});
});

describe("icpcAssignRanks", () => {
	it("assigns sequential ranks when no ties exist", () => {
		const entries: IIcpcEntry[] = [
			{ id: "A", solvedProblems: 3, penaltyCounts: 0, timeUsing: 1000 },
			{ id: "B", solvedProblems: 2, penaltyCounts: 0, timeUsing: 1000 },
			{ id: "C", solvedProblems: 1, penaltyCounts: 0, timeUsing: 1000 },
		];

		const ranked = icpcAssignRanks(entries, defaultOptions);
		const ranks = ranked.map((r) => r.rank);

		expect(ranks).toEqual([1, 2, 3]);
		expect(ranked.map((r) => r.entry.id)).toEqual(["A", "B", "C"]);
	});

	it("assigns tied ranks as 1,2,2,4 style", () => {
		const entries: IIcpcEntry[] = [
			{ id: "A", solvedProblems: 5, penaltyCounts: 0, timeUsing: 2000 },
			{ id: "B", solvedProblems: 5, penaltyCounts: 0, timeUsing: 2000 }, // tie with A
			{ id: "C", solvedProblems: 5, penaltyCounts: 1, timeUsing: 1000 }, // same penalty: 1000 + 1200 = 2200, so not tie with A/B
			{ id: "D", solvedProblems: 3, penaltyCounts: 0, timeUsing: 500 },
		];

		const ranked = icpcAssignRanks(entries, defaultOptions);

		expect(ranked[0]!.rank).toBe(1);
		expect(ranked[0]!.entry.id).toBe("A");
		expect(ranked[1]!.rank).toBe(1); // tied with A
		expect(ranked[1]!.entry.id).toBe("B");
		expect(ranked[2]!.rank).toBe(3); // skips 2
		expect(ranked[2]!.entry.id).toBe("C");
		expect(ranked[3]!.rank).toBe(4);
		expect(ranked[3]!.entry.id).toBe("D");
	});

	it("assigns same rank for teams tied on both `solvedProblems` and penalty", () => {
		const entries: IIcpcEntry[] = [
			{ id: "X", solvedProblems: 4, penaltyCounts: 2, timeUsing: 6000 },
			{ id: "Y", solvedProblems: 4, penaltyCounts: 2, timeUsing: 6000 },
		];

		const ranked = icpcAssignRanks(entries, defaultOptions);

		expect(ranked[0]!.rank).toBe(1);
		expect(ranked[1]!.rank).toBe(1);
	});

	it("does not treat different penalties as tie even with same `solvedProblems`", () => {
		const entries: IIcpcEntry[] = [
			{ id: "Fast", solvedProblems: 3, penaltyCounts: 0, timeUsing: 1000 },
			{ id: "Slow", solvedProblems: 3, penaltyCounts: 0, timeUsing: 2000 },
		];

		const ranked = icpcAssignRanks(entries, defaultOptions);

		expect(ranked[0]!.rank).toBe(1);
		expect(ranked[1]!.rank).toBe(2);
	});

	it("assigns rank 1 to all when all entries are tied", () => {
		const entries: IIcpcEntry[] = [
			{ id: "A", solvedProblems: 1, penaltyCounts: 0, timeUsing: 100 },
			{ id: "B", solvedProblems: 1, penaltyCounts: 0, timeUsing: 100 },
			{ id: "C", solvedProblems: 1, penaltyCounts: 0, timeUsing: 100 },
		];

		const ranked = icpcAssignRanks(entries, defaultOptions);

		expect(ranked.every((r) => r.rank === 1)).toBe(true);
	});

	it("preserves sort order in the returned entries", () => {
		const entries: IIcpcEntry[] = [
			{ id: "Low", solvedProblems: 1, penaltyCounts: 0, timeUsing: 100 },
			{ id: "Mid", solvedProblems: 2, penaltyCounts: 0, timeUsing: 200 },
			{ id: "High", solvedProblems: 3, penaltyCounts: 0, timeUsing: 300 },
		];

		const ranked = icpcAssignRanks(entries, defaultOptions);

		expect(ranked.map((r) => r.entry.id)).toEqual(["High", "Mid", "Low"]);
	});

	it("handles empty entries array", () => {
		expect(icpcAssignRanks([], defaultOptions)).toEqual([]);
	});

	it("handles a single entry", () => {
		const ranked = icpcAssignRanks(
			[{ id: "One", solvedProblems: 1, penaltyCounts: 0, timeUsing: 0 }],
			defaultOptions,
		);

		expect(ranked).toEqual([
			{
				entry: { id: "One", solvedProblems: 1, penaltyCounts: 0, timeUsing: 0 },
				rank: 1,
			},
		]);
	});

	it("multi-way tie then gap then another tie", () => {
		const entries: IIcpcEntry[] = [
			{ id: "A", solvedProblems: 5, penaltyCounts: 0, timeUsing: 5000 },
			{ id: "B", solvedProblems: 5, penaltyCounts: 0, timeUsing: 5000 },
			{ id: "C", solvedProblems: 5, penaltyCounts: 0, timeUsing: 5000 }, // 3-way tie at rank 1
			{ id: "D", solvedProblems: 4, penaltyCounts: 0, timeUsing: 4000 }, // rank 4
			{ id: "E", solvedProblems: 4, penaltyCounts: 0, timeUsing: 4000 }, // rank 4
		];

		const ranked = icpcAssignRanks(entries, defaultOptions);

		expect(ranked.map((r) => r.rank)).toEqual([1, 1, 1, 4, 4]);
	});
});
