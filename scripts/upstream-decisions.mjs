// What we decided about each upstream change that landed on top of ours.
//
// WHY THIS EXISTS, AND WHY IT IS THE ONLY REGISTER
//
// Merging used to erase the review window. The review ran from the git
// merge-base, so the moment the merge landed the base moved and the script said
// "nothing new" — whether or not anybody had looked. The CHANGELOG line meant to
// force the review could be satisfied with one `sed`.
//
// So the reviewed-through point lives here, with the decisions attached to it,
// and the CHANGELOG line is checked against this file rather than written
// independently. There is one register, not two, which is the whole objection to
// keeping a register in the first place: two of them drift, and the stale
// CHANGELOG line was the proof.
//
// HOW IT IS ENFORCED
//
// check-mergeability.mjs re-derives the overlaps in the range the newest entry
// covers — `REVIEWS[n-1].through .. REVIEWS[n].through` — and fails until every
// gated one has a decision here. You cannot advance `through` without the script
// asking again for each overlap it finds. It cannot check that the reasoning is
// any good; it can make sure the question was put.
//
// Only the newest entry's range is re-derived. Older ones are records of what
// was known then: re-deriving them with today's SHADOWED list and today's borrow
// markers would invent overlaps nobody could have seen at the time.
//
// VERDICTS
//
//   adopt           take theirs, drop ours
//   keep-ours       ours stands; theirs is knowingly not used here
//   combine         both, reconciled by hand — say how
//   not-applicable  the overlap is mechanical noise; say why it cannot bite
//
// Run `npm run review:upstream` — it prints the overlap keys ready to paste.

export const REVIEWS = [
  {
    through: 'ef25ba2af99b6b0da1c568f51ccbb9040162bfc6',
    date: '2026-09-13',
    decisions: [],
    note:
      'Seed entry. This is where the fork stood when overlap review was built, '
      + 'so there is no earlier entry to derive a range from and nothing is '
      + 'claimed about the ten commits merged before it. The first real review '
      + 'is the next one.',
  },
];

/** The commit every upstream change up to which has been reviewed. */
export const reviewedThrough = () => REVIEWS[REVIEWS.length - 1].through;

/** The entry being added, and the range it must account for. */
export const newestRange = () => ({
  from: REVIEWS.length > 1 ? REVIEWS[REVIEWS.length - 2].through : null,
  to: reviewedThrough(),
  entry: REVIEWS[REVIEWS.length - 1],
});
