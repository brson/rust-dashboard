use std::collections::BTreeMap;

use chrono::{NaiveDate, NaiveDateTime};
use diesel::expression::dsl::*;
use diesel::expression::AsExpression;
use diesel::prelude::*;
use diesel::types::{BigInt, Date, Double, Integer, Text};

use DB_POOL;
use domain::releases::Release;
use error::DashResult;

pub type EpochTimestamp = i64;

#[derive(Clone, Debug, Serialize)]
pub struct PullRequestSummary {
    opened_per_day: Vec<(EpochTimestamp, i64)>,
    closed_per_day: Vec<(EpochTimestamp, i64)>,
    merged_per_day: Vec<(EpochTimestamp, i64)>,
    days_open_before_close: Vec<(EpochTimestamp, f64)>,
    current_open_age_days_mean: f64,
    bors_retries: Vec<(i32, i64)>,
}

#[derive(Clone, Debug, Serialize)]
pub struct IssueSummary {
    opened_per_day: Vec<(EpochTimestamp, i64)>,
    closed_per_day: Vec<(EpochTimestamp, i64)>,
    days_open_before_close: Vec<(EpochTimestamp, f64)>,
    current_open_age_days_mean: f64,
    num_open_p_high_issues: i64,
    num_open_regression_nightly_issues: i64,
    num_open_regression_beta_issues: i64,
    num_open_regression_stable_issues: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReleaseSummary {
    nightlies: Vec<Release>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BuildbotSummary {
    per_builder_times_mins: Vec<(String, Vec<(EpochTimestamp, f64)>)>,
    per_builder_failures: Vec<(String, Vec<(EpochTimestamp, i64)>)>,
}

pub fn issue_summary(since: NaiveDate, until: NaiveDate) -> DashResult<IssueSummary> {
    let since = since.and_hms(0, 0, 0);
    let until = until.and_hms(23, 59, 59);

    let current_issue_age = try!(open_issues_avg_days_old());
    let issue_open_time = try!(issues_open_time_before_close(since, until));
    let issues_open_per_day = try!(issues_opened_per_day(since, until));
    let issues_close_per_day = try!(issues_closed_per_day(since, until));
    let num_p_high = try!(open_issues_with_label("P-high"));
    let nightly_regress = try!(open_issues_with_label("regression-from-stable-to-nightly"));
    let beta_regress = try!(open_issues_with_label("regression-from-stable-to-beta"));
    let stable_regress = try!(open_issues_with_label("regression-from-stable-to-stable"));

    Ok(IssueSummary {
        opened_per_day: issues_open_per_day,
        closed_per_day: issues_close_per_day,
        days_open_before_close: issue_open_time,
        current_open_age_days_mean: current_issue_age,
        num_open_p_high_issues: num_p_high,
        num_open_regression_nightly_issues: nightly_regress,
        num_open_regression_beta_issues: beta_regress,
        num_open_regression_stable_issues: stable_regress,
    })
}

pub fn pr_summary(since: NaiveDate, until: NaiveDate) -> DashResult<PullRequestSummary> {
    let since = since.and_hms(0, 0, 0);
    let until = until.and_hms(23, 59, 59);

    let current_pr_age = try!(open_prs_avg_days_old());
    let prs_open_per_day = try!(prs_opened_per_day(since, until));
    let prs_close_per_day = try!(prs_closed_per_day(since, until));
    let prs_merge_per_day = try!(prs_merged_per_day(since, until));
    let pr_open_time = try!(prs_open_time_before_close(since, until));
    let bors_retries = try!(bors_retries_per_pr(since, until));

    Ok(PullRequestSummary {
        opened_per_day: prs_open_per_day,
        closed_per_day: prs_close_per_day,
        merged_per_day: prs_merge_per_day,
        days_open_before_close: pr_open_time,
        current_open_age_days_mean: current_pr_age,
        bors_retries: bors_retries,
    })
}

pub fn ci_summary(since: NaiveDate, until: NaiveDate) -> DashResult<BuildbotSummary> {
    let since = since.and_hms(0, 0, 0);
    let until = until.and_hms(23, 59, 59);

    let per_builder_times = try!(buildbot_build_times(since, until));
    let per_builder_fails = try!(buildbot_failures_by_day(since, until));

    Ok(BuildbotSummary {
        per_builder_times_mins: per_builder_times,
        per_builder_failures: per_builder_fails,
    })
}

pub fn release_summary(since: NaiveDate, until: NaiveDate) -> DashResult<ReleaseSummary> {
    let since = since.and_hms(0, 0, 0);
    let until = until.and_hms(23, 59, 59);

    Ok(ReleaseSummary { nightlies: try!(nightly_releases(since, until)) })
}

pub fn prs_opened_per_day(since: NaiveDateTime,
                          until: NaiveDateTime)
                          -> DashResult<Vec<(EpochTimestamp, i64)>> {
    use domain::schema::pullrequest::dsl::*;

    let conn = try!(DB_POOL.get());
    let d = sql::<Date>("d");
    Ok(try!(pullrequest.select(sql::<(Date, BigInt)>("created_at::date as d, COUNT(*)"))
            .filter(created_at.ge(since))
            .filter(created_at.le(until))
            .group_by(d)
            .order(date(created_at).asc())
            .get_results::<(NaiveDate, i64)>(&*conn))
        .into_iter()
        .map(|(d, cnt)| (d.and_hms(12, 0, 0).timestamp(), cnt))
        .collect())
}

pub fn prs_closed_per_day(since: NaiveDateTime,
                          until: NaiveDateTime)
                          -> DashResult<Vec<(EpochTimestamp, i64)>> {
    use domain::schema::pullrequest::dsl::*;

    let conn = try!(DB_POOL.get());
    let d = sql::<Date>("d");
    Ok(try!(pullrequest.select(sql::<(Date, BigInt)>("closed_at::date as d, COUNT(*)"))
            .filter(closed_at.is_not_null())
            .filter(closed_at.ge(since))
            .filter(closed_at.le(until))
            .group_by(&d)
            .order((&d).asc())
            .get_results::<(NaiveDate, i64)>(&*conn))
        .into_iter()
        .map(|(d, cnt)| (d.and_hms(12, 0, 0).timestamp(), cnt))
        .collect())
}

pub fn prs_merged_per_day(since: NaiveDateTime,
                          until: NaiveDateTime)
                          -> DashResult<Vec<(EpochTimestamp, i64)>> {
    use domain::schema::pullrequest::dsl::*;

    let conn = try!(DB_POOL.get());
    let d = sql::<Date>("d");
    Ok(try!(pullrequest.select(sql::<(Date, BigInt)>("merged_at::date as d, COUNT(*)"))
            .filter(merged_at.is_not_null())
            .filter(merged_at.ge(since))
            .filter(merged_at.le(until))
            .group_by(&d)
            .order((&d).asc())
            .get_results::<(NaiveDate, i64)>(&*conn))
        .into_iter()
        .map(|(d, cnt)| (d.and_hms(12, 0, 0).timestamp(), cnt))
        .collect())
}

pub fn prs_open_time_before_close(since: NaiveDateTime,
                                  until: NaiveDateTime)
                                  -> DashResult<Vec<(EpochTimestamp, f64)>> {
    use domain::schema::pullrequest::dsl::*;

    let conn = try!(DB_POOL.get());

    let w = sql::<Text>("iso_closed_week");
    let mut results = try!(pullrequest.select(sql::<(Double, Text)>("\
        AVG(EXTRACT(EPOCH FROM closed_at) - \
                                           EXTRACT(EPOCH FROM created_at)) / (60 * 60 * 24), \
                                           \
                                           EXTRACT(ISOYEAR FROM closed_at)::text || '-' || \
                                           EXTRACT(WEEK FROM closed_at)::text || '-6' AS \
                                           iso_closed_week"))
            .filter(closed_at.is_not_null())
            .filter(closed_at.ge(since))
            .filter(closed_at.le(until))
            .group_by(&w)
            .get_results::<(f64, String)>(&*conn))
        .into_iter()
        .map(|(time, week)| {
            let d = NaiveDate::parse_from_str(&week, "%G-%V-%w").unwrap();
            let d = d.and_hms(12, 0, 0).timestamp();
            (d, time)
        })
        .collect::<Vec<(EpochTimestamp, f64)>>();

    results.sort_by(|&(d1, _), &(d2, _)| d1.cmp(&d2));
    Ok(results)
}

pub fn open_prs_avg_days_old() -> DashResult<f64> {
    use domain::schema::pullrequest::dsl::*;
    let conn = try!(DB_POOL.get());
    Ok(try!(pullrequest.select(sql::<Double>("AVG(EXTRACT(EPOCH FROM (now() - created_at))) / \
                                              (60 * 60 * 24)"))
                       .filter(closed_at.is_null())
                       .first(&*conn)))
}

pub fn bors_retries_per_pr(since: NaiveDateTime,
                           until: NaiveDateTime)
                           -> DashResult<Vec<(i32, i64)>> {

    use domain::schema::issuecomment::dsl::*;
    let conn = try!(DB_POOL.get());

    Ok(try!(issuecomment.select(sql::<(Integer, BigInt)>("fk_issue, COUNT(*)"))
            .filter(body.like("%@bors%retry%"))
            .filter(created_at.ge(since))
            .filter(created_at.le(until))
            .group_by(fk_issue)
            .order(count_star().desc())
            .load(&*conn))
        .into_iter()
        .collect())
}

pub fn issues_opened_per_day(since: NaiveDateTime,
                             until: NaiveDateTime)
                             -> DashResult<Vec<(EpochTimestamp, i64)>> {
    use domain::schema::issue::dsl::*;

    let conn = try!(DB_POOL.get());
    let d = sql::<Date>("d");
    Ok(try!(issue.select(sql::<(Date, BigInt)>("created_at::date as d, COUNT(*)"))
            .filter(created_at.ge(since))
            .filter(created_at.le(until))
            .group_by(&d)
            .order((&d).asc())
            .get_results::<(NaiveDate, i64)>(&*conn))
        .into_iter()
        .map(|(t, c)| (t.and_hms(12, 0, 0).timestamp(), c))
        .collect())
}

pub fn issues_closed_per_day(since: NaiveDateTime,
                             until: NaiveDateTime)
                             -> DashResult<Vec<(EpochTimestamp, i64)>> {
    use domain::schema::issue::dsl::*;

    let conn = try!(DB_POOL.get());
    let d = sql::<Date>("d");
    Ok(try!(issue.select(sql::<(Date, BigInt)>("closed_at::date as d, COUNT(*)"))
            .filter(closed_at.is_not_null())
            .filter(closed_at.ge(since))
            .filter(closed_at.le(until))
            .group_by(&d)
            .order((&d).asc())
            .get_results::<(NaiveDate, i64)>(&*conn))
        .into_iter()
        .map(|(t, c)| (t.and_hms(12, 0, 0).timestamp(), c))
        .collect())
}

pub fn issues_open_time_before_close(since: NaiveDateTime,
                                     until: NaiveDateTime)
                                     -> DashResult<Vec<(EpochTimestamp, f64)>> {
    use domain::schema::issue::dsl::*;

    let conn = try!(DB_POOL.get());

    let w = sql::<Text>("iso_closed_week");
    let mut results = try!(issue.select(sql::<(Double, Text)>("\
                                             \
                                           AVG(EXTRACT(EPOCH FROM closed_at) - EXTRACT(EPOCH \
                                           FROM created_at)) / (60 * 60 * 24), \
                                           \
                                           EXTRACT(ISOYEAR FROM closed_at)::text || '-' || \
                                           EXTRACT(WEEK FROM closed_at)::text || '-6' AS \
                                           iso_closed_week"))
            .filter(closed_at.is_not_null())
            .filter(closed_at.ge(since))
            .filter(closed_at.le(until))
            .group_by(&w)
            .get_results::<(f64, String)>(&*conn))
        .into_iter()
        .map(|(time, week)| {
            let d = NaiveDate::parse_from_str(&week, "%G-%V-%w").unwrap();
            let d = d.and_hms(12, 0, 0).timestamp();
            (d, time)
        })
        .collect::<Vec<(EpochTimestamp, f64)>>();

    results.sort_by(|&(d1, _), &(d2, _)| d1.cmp(&d2));
    Ok(results)
}

pub fn open_issues_avg_days_old() -> DashResult<f64> {
    use domain::schema::issue::dsl::*;
    let conn = try!(DB_POOL.get());
    Ok(try!(issue.select(sql::<Double>("AVG(EXTRACT(EPOCH FROM (now() - created_at))) / \
                                              (60 * 60 * 24)"))
                 .filter(closed_at.is_null())
                 .first(&*conn)))
}

pub fn open_issues_with_label(label: &str) -> DashResult<i64> {
    use domain::schema::issue::dsl::*;
    let conn = try!(DB_POOL.get());

    Ok(try!(issue.select(count_star())
        .filter(closed_at.is_null())
        .filter(AsExpression::<Text>::as_expression(label).eq(any(labels)))
        .first(&*conn)))
}

pub fn buildbot_build_times(since: NaiveDateTime,
                            until: NaiveDateTime)
                            -> DashResult<Vec<(String, Vec<(EpochTimestamp, f64)>)>> {
    use domain::schema::build::dsl::*;

    let conn = try!(DB_POOL.get());

    let name_date = sql::<(Text, Date)>("builder_name, date(start_time)");

    let triples = try!(build.select((&name_date, sql::<Double>("(AVG(duration_secs) / 60)::float")))
        .filter(successful)
        .filter(start_time.is_not_null())
        .filter(start_time.ge(since))
        .filter(start_time.le(until))
        .filter(builder_name.like("auto-%"))
        .group_by(&name_date)
        .order((&name_date).asc())
        .load::<((String, NaiveDate), f64)>(&*conn));

    let mut results = BTreeMap::new();
    for ((builder, date), build_minutes) in triples {
        results.entry(builder)
            .or_insert(Vec::new())
            .push((date.and_hms(12, 0, 0).timestamp(), build_minutes));
    }

    Ok(results.into_iter().collect())
}

pub fn buildbot_failures_by_day(since: NaiveDateTime,
                                until: NaiveDateTime)
                                -> DashResult<Vec<(String, Vec<(EpochTimestamp, i64)>)>> {
    use domain::schema::build::dsl::*;

    let conn = try!(DB_POOL.get());

    let name_date = sql::<(Text, Date)>("builder_name, date(start_time)");

    let triples = try!(build.select((&name_date, sql::<BigInt>("COUNT(*)")))
        .filter(successful.ne(true))
        .filter(start_time.is_not_null())
        .filter(start_time.ge(since))
        .filter(start_time.le(until))
        .filter(builder_name.like("auto-%"))
        .group_by(&name_date)
        .order((&name_date).asc())
        .load::<((String, NaiveDate), i64)>(&*conn));

    let mut results = BTreeMap::new();
    for ((builder, date), build_minutes) in triples {
        results.entry(builder)
            .or_insert(Vec::new())
            .push((date.and_hms(12, 0, 0).timestamp(), build_minutes));
    }

    Ok(results.into_iter().collect())
}

pub fn nightly_releases(since: NaiveDateTime, until: NaiveDateTime) -> DashResult<Vec<Release>> {
    use domain::schema::release::dsl::*;

    let conn = try!(DB_POOL.get());

    Ok(try!(release.select((date, released))
        .filter(date.gt(since.date()))
        .filter(date.le(until.date()))
        .order(date.desc())
        .load::<Release>(&*conn)))
}
