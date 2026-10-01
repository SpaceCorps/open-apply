//! Core domain types: job statuses, event types, and the job and event rows.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Lead,
    Saved,
    Ready,
    Applied,
    Acknowledged,
    Screening,
    Interview,
    Assessment,
    Offer,
    Accepted,
    Rejected,
    Ghosted,
    Withdrawn,
    Closed,
}

impl Status {
    pub const ALL: [Status; 14] = [
        Status::Lead,
        Status::Saved,
        Status::Ready,
        Status::Applied,
        Status::Acknowledged,
        Status::Screening,
        Status::Interview,
        Status::Assessment,
        Status::Offer,
        Status::Accepted,
        Status::Rejected,
        Status::Ghosted,
        Status::Withdrawn,
        Status::Closed,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Status::Lead => "lead",
            Status::Saved => "saved",
            Status::Ready => "ready",
            Status::Applied => "applied",
            Status::Acknowledged => "acknowledged",
            Status::Screening => "screening",
            Status::Interview => "interview",
            Status::Assessment => "assessment",
            Status::Offer => "offer",
            Status::Accepted => "accepted",
            Status::Rejected => "rejected",
            Status::Ghosted => "ghosted",
            Status::Withdrawn => "withdrawn",
            Status::Closed => "closed",
        }
    }

    pub fn parse(s: &str) -> Result<Status> {
        let lower = s.trim().to_ascii_lowercase();
        Status::ALL.iter().copied().find(|st| st.as_str() == lower).ok_or_else(|| {
            Error::validation(format!("unknown status '{s}'"))
                .hint(format!("valid statuses: {}", Status::ALL.map(Status::as_str).join(", ")))
        })
    }

    /// Position in the pipeline. An inbound event only moves a job forward along this order.
    pub fn rank(self) -> u8 {
        match self {
            Status::Lead => 0,
            Status::Saved => 1,
            Status::Ready => 2,
            Status::Applied | Status::Ghosted => 3,
            Status::Acknowledged => 4,
            Status::Screening => 5,
            Status::Assessment => 6,
            Status::Interview => 7,
            Status::Offer => 8,
            Status::Accepted => 9,
            Status::Rejected | Status::Withdrawn | Status::Closed => 10,
        }
    }

    /// Terminal statuses are never moved by inbound events.
    pub fn is_terminal(self) -> bool {
        matches!(self, Status::Accepted | Status::Rejected | Status::Withdrawn | Status::Closed)
    }

    /// Statuses that mean an application exists and is still in flight.
    pub fn is_active_application(self) -> bool {
        matches!(
            self,
            Status::Applied
                | Status::Acknowledged
                | Status::Screening
                | Status::Assessment
                | Status::Interview
                | Status::Offer
        )
    }

    /// Statuses `next` offers to the agent as work.
    pub fn is_queue(self) -> bool {
        matches!(self, Status::Lead | Status::Saved | Status::Ready)
    }

    /// The event type recorded when a job is moved to this status by hand.
    pub fn event_type(self) -> EventType {
        match self {
            Status::Applied => EventType::Applied,
            Status::Acknowledged => EventType::Ack,
            Status::Screening => EventType::Screen,
            Status::Interview => EventType::Interview,
            Status::Assessment => EventType::Assessment,
            Status::Offer => EventType::Offer,
            Status::Rejected => EventType::Rejection,
            _ => EventType::Status,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    Created,
    Applied,
    Status,
    Ack,
    Screen,
    Interview,
    Assessment,
    Offer,
    Rejection,
    FollowUp,
    Note,
}

impl EventType {
    pub const USER_ADDABLE: [EventType; 8] = [
        EventType::Ack,
        EventType::Screen,
        EventType::Interview,
        EventType::Assessment,
        EventType::Offer,
        EventType::Rejection,
        EventType::FollowUp,
        EventType::Note,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            EventType::Created => "created",
            EventType::Applied => "applied",
            EventType::Status => "status",
            EventType::Ack => "ack",
            EventType::Screen => "screen",
            EventType::Interview => "interview",
            EventType::Assessment => "assessment",
            EventType::Offer => "offer",
            EventType::Rejection => "rejection",
            EventType::FollowUp => "follow_up",
            EventType::Note => "note",
        }
    }

    pub fn parse(s: &str) -> Result<EventType> {
        let lower = s.trim().to_ascii_lowercase().replace('-', "_");
        [EventType::Created, EventType::Applied, EventType::Status]
            .into_iter()
            .chain(EventType::USER_ADDABLE)
            .find(|t| t.as_str() == lower)
            .ok_or_else(|| {
                Error::validation(format!("unknown event type '{s}'"))
                    .hint(format!("valid types: {}", EventType::USER_ADDABLE.map(EventType::as_str).join(", ")))
            })
    }

    /// An employer answering (anything from an acknowledgement to a rejection). Drives response stats.
    pub fn is_response(self) -> bool {
        matches!(
            self,
            EventType::Ack
                | EventType::Screen
                | EventType::Interview
                | EventType::Assessment
                | EventType::Offer
                | EventType::Rejection
        )
    }

    /// The status this event type moves a job towards, if any.
    pub fn target_status(self) -> Option<Status> {
        match self {
            EventType::Ack => Some(Status::Acknowledged),
            EventType::Screen => Some(Status::Screening),
            EventType::Interview => Some(Status::Interview),
            EventType::Assessment => Some(Status::Assessment),
            EventType::Offer => Some(Status::Offer),
            EventType::Rejection => Some(Status::Rejected),
            _ => None,
        }
    }
}

/// What status, if any, an inbound event should move a job to. Progress only moves forward, so a
/// late acknowledgement cannot drag an interview back; a rejection ends any non-terminal job.
pub fn advance(current: Status, event: EventType) -> Option<Status> {
    let target = event.target_status()?;
    if current.is_terminal() || target == current {
        return None;
    }
    if target == Status::Rejected || target.rank() > current.rank() { Some(target) } else { None }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Via {
    Linkedin,
    Ats,
    Email,
    Referral,
    Other,
}

impl Via {
    pub fn as_str(self) -> &'static str {
        match self {
            Via::Linkedin => "linkedin",
            Via::Ats => "ats",
            Via::Email => "email",
            Via::Referral => "referral",
            Via::Other => "other",
        }
    }

    pub fn parse(s: &str) -> Result<Via> {
        match s.trim().to_ascii_lowercase().as_str() {
            "linkedin" => Ok(Via::Linkedin),
            "ats" => Ok(Via::Ats),
            "email" => Ok(Via::Email),
            "referral" => Ok(Via::Referral),
            "other" => Ok(Via::Other),
            _ => Err(Error::validation(format!("unknown channel '{s}'"))
                .hint("valid values: linkedin, ats, email, referral, other")),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub url: String,
    pub canonical_url: String,
    pub title: String,
    pub company: String,
    pub location: String,
    pub remote: Option<bool>,
    pub source: String,
    pub ats: Option<String>,
    pub description: String,
    pub notes: String,
    pub status: Status,
    pub tracking_only: bool,
    pub cv_path: Option<String>,
    pub cover_path: Option<String>,
    pub posted_at: Option<String>,
    pub applied_at: Option<String>,
    pub applied_via: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl Job {
    pub fn company_key(&self) -> String {
        crate::util::company_key(&self.company)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub id: i64,
    pub job_id: String,
    #[serde(rename = "type")]
    pub kind: EventType,
    pub status: Option<Status>,
    pub note: Option<String>,
    pub at: String,
    pub source: String,
    pub recorded_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_round_trip() {
        for s in Status::ALL {
            assert_eq!(Status::parse(s.as_str()).unwrap(), s);
        }
        assert_eq!(Status::parse(" Interview ").unwrap(), Status::Interview);
        assert!(Status::parse("hired").is_err());
    }

    #[test]
    fn event_type_parse() {
        assert_eq!(EventType::parse("follow-up").unwrap(), EventType::FollowUp);
        assert_eq!(EventType::parse("ACK").unwrap(), EventType::Ack);
        assert!(EventType::parse("ghost").is_err());
    }

    #[test]
    fn advance_moves_forward_only() {
        assert_eq!(advance(Status::Applied, EventType::Ack), Some(Status::Acknowledged));
        assert_eq!(advance(Status::Acknowledged, EventType::Screen), Some(Status::Screening));
        assert_eq!(advance(Status::Interview, EventType::Ack), None);
        assert_eq!(advance(Status::Interview, EventType::Screen), None);
        assert_eq!(advance(Status::Interview, EventType::Assessment), None);
        assert_eq!(advance(Status::Ghosted, EventType::Ack), Some(Status::Acknowledged));
        assert_eq!(advance(Status::Applied, EventType::Note), None);
        assert_eq!(advance(Status::Applied, EventType::FollowUp), None);
    }

    #[test]
    fn rejection_ends_live_jobs_but_terminal_is_sticky() {
        assert_eq!(advance(Status::Interview, EventType::Rejection), Some(Status::Rejected));
        assert_eq!(advance(Status::Offer, EventType::Rejection), Some(Status::Rejected));
        assert_eq!(advance(Status::Rejected, EventType::Offer), None);
        assert_eq!(advance(Status::Withdrawn, EventType::Rejection), None);
        assert_eq!(advance(Status::Accepted, EventType::Rejection), None);
    }

    #[test]
    fn manual_status_maps_to_event_type() {
        assert_eq!(Status::Interview.event_type(), EventType::Interview);
        assert_eq!(Status::Rejected.event_type(), EventType::Rejection);
        assert_eq!(Status::Ghosted.event_type(), EventType::Status);
        assert!(Status::Rejected.event_type().is_response());
        assert!(!Status::Ghosted.event_type().is_response());
    }
}
